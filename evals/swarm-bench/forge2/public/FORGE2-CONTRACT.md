# Scope Ledger 2 — Forge app contract (forge-2.0)

Scope Ledger v1 is installed on a large Jira Cloud site and has been in use for days: agile coaches see on a
dashboard, from the sprint itself and from Rovo what entered a sprint after it started, who added it and how many
story points it carried. This workspace is v1's source (`STARTER.md`). Ship v2: everything v1 does (§1–§8, which
keep their rules unless a later section changes them) plus the guarantees of §9–§18: a live migration of v1's data,
a points quota, time limits, a changing world, a UI Kit admin panel, signed CI events, a custom field, safer Forge
LLM use and a Custom UI boot budget.

The harness upgrades the installation from v1 to your v2 over v1's stored data — that instant is virtual hour 0 —
and runs it for 6 virtual hours on a seeded site: product events, queue deliveries, scheduled runs, resolver calls
from every surface as different people, the Rovo action, CI deployment events, admin actions, and changes to sprints,
boards, issues and permissions. It grades the app by running it. Nothing is deployed.

**Scale** (scoring sites; the dev site has the same shape and another seed): 3 projects; 4 scrum boards, 2 estimating
with one field and 2 with another (field ids vary per seed); 6 active sprints, 2 future, 6 closed; about 1,000
issues, about 300 of them in active sprints; about 200 relevant changes and about 800 irrelevant issue updates over the
6 scored hours.

## 1. The numbers

For each **active** sprint S, with `startDate` from the Jira Software sprint API:

- A **change** is a Sprint-field changelog entry created after `startDate` that puts an issue into S (`added`) or
  takes it out of S (`removed`). Its id is the changelog id, its time the changelog `created`, its author the
  changelog author. A change is keyed by changelog id + sprint: one entry moving an issue between two sprints is a
  `removed` in one and an `added` in the other.
- **committed** = sum of current estimates of issues that were in S at `startDate`.
- **added** = sum of current estimates of issues in S now that were not in S at `startDate`.
- **removed** = sum of current estimates of issues in S at some time after `startDate` and not in S now.
- **creep** = `100 × added / committed`, rounded half away from zero to one decimal and always written with that
  decimal and `%` (`20.0%`); `—` when committed is 0.
- An issue's **estimate** for S is the value of the estimation field S's board (the sprint's `originBoardId`) uses
  now; no value counts as 0, and a deleted issue has no value. "After `startDate`" is strictly after.
- Points are written as plain decimals (`34.5`, `0`), no thousands separators.

Background work (triggers, consumers, scheduled jobs, the web trigger) sees every issue. What a **person** sees in the
sprint action and the Rovo action lists only changes to issues that person can browse, plus a count of the changes
hidden from them; the totals above are team totals and are the same for everyone. Changes of deleted issues are
listed to nobody and counted as hidden for nobody.

## 2. Modules

| module | requirement |
|---|---|
| `trigger` | on `avi:jira:updated:issue`; hands relevant work to a queue and returns |
| `consumer` | Forge async events queue consumer; performs the ledger writes |
| `scheduledTrigger` | `interval: hour`; backfill and reconciliation (§3) |
| `dashboards:widget` | Custom UI view + `edit` (§4); the legacy `jira:dashboardGadget` earns nothing |
| `jira:sprintAction` | Custom UI modal (§5) |
| `action` | Rovo action `get-sprint-scope` (§6) |
| `rovo:skill` | `skills/sprint-scope-analyst/`, depends on `get-sprint-scope` (§6) |
| `rovo:agent` | lists the skill |
| `rovo:mcp` | one module, `name` at most 30 characters, exposing `get-sprint-scope` |
| `llm` | Forge LLM (`model: [claude]`), for the sprint action's explanation (§5, §16) |
| `jira:adminPage` | the admin panel in UI Kit: `render: native`, `@forge/react` (§13) |
| `webtrigger` | static (`response.type: static`); receives CI deployment events (§14) |
| `jira:customField` | key `scope-status`, `type: string`, `readOnly: true` (§15) |

Resolvers use `@forge/resolver`. Every resolver returns a value and never throws: a failure (a Jira error, storage,
Forge LLM, a refused permission) returns a value describing it, e.g. `{ "error": "…" }`, and the surface shows it.
Storage is Forge KVS: the v2 ledger lives in the custom entity `scope-ledger` (§9), indexed by sprint (partition) and
change time (range), and is read back through that index. Request only the scopes your calls need: per call, the
OAuth2 scopes the shipped OpenAPI lists for it (the classic scope where one exists, else the whole granular set). The
linter does not see every call. The widget and the sprint action are Custom UI; the admin page is UI Kit; a UI Kit
widget or sprint action earns nothing.

## 3. Backend behaviour

- The site existed long before v2. v1's ledger holds the changes of each active sprint's first 2 days (§9). After
  the upgrade the harness runs your scheduled triggers once, at virtual hour 0, before it delivers any update, then
  once per virtual hour. The first run starts the backfill of every change since each active sprint started that
  the ledger lacks, including changes of issues that have since left every sprint.
- Issue updates keep the ledger current: sprint changes add ledger rows; estimate changes move the numbers. Updates
  that touch neither do no Jira or queue work (storage reads are fine).
- Product events can arrive more than once and out of order, and some never arrive. The ledger holds **exactly one
  row per change**, whatever the delivery history. Each row records `source`: `event` or `reconcile`, the path that
  recorded it first (event work may also record other changes of the issue it reads); a migrated row keeps v1's.
  Later scheduled runs record what the event stream missed; a run with nothing new writes nothing.
- Background work uses `asApp()`. Whatever shows a person issue data (keys, authors, change lists, explanations) shows
  only what that person can browse at the time of the request — read as them (`asUser()`), or as the app with an
  explicit permission check for them. Comments are posted as the person. Team totals come from the ledger.

## 4. Dashboard widget

**Edit** (`edit.resource`): one element per scrum board, `[data-testid="board-option"]` with `data-board-id`,
clickable; the selected one carries `aria-pressed="true"`. The choice reaches the dashboard through the dashboards
widget edit API (`@forge/dashboards-bridge`). The dashboard's own Save calls your `onProductSave` handler with the last
`updateConfig` value (the stored config if none was sent) and stores what it returns (`null` stores nothing); with no
handler registered it stores the last `updateConfig` value. Reopening edit shows the stored board selected.

**View** (`resource`), root `[data-testid="scope-widget"]`. It takes its board from the widget configuration in its
context, so two widgets on one dashboard can show different boards:
- No stored board: `[data-testid="needs-config"]`, nothing else.
- Otherwise one `[data-testid="sprint"][data-sprint-id="<id>"]` per active sprint of that board, ordered by
  `startDate` (ties by sprint id), each holding `[data-metric="committed"]`, `[data-metric="added"]`,
  `[data-metric="removed"]` and `[data-metric="creep"]` whose text is the §1 number.
- A bar chart: one `<svg data-testid="chart">`; per sprint and per series one
  `<rect data-sprint-id data-series="committed|added|removed">`; every bar on one linear scale from 0 (rendered
  height proportional to its number within 1 px).
- At 380 px wide nothing scrolls horizontally and no number is clipped or truncated (long names may end in an
  ellipsis).
- Live: after ledger rows are written, an open widget shows the new numbers without a reload, through Forge Realtime
  (`@forge/realtime` in the backend, the bridge's `realtime` in the widget) — no polling. A `publishGlobal` reaches the
  `subscribeGlobal` subscriptions on its channel; a `publish` reaches `subscribe` subscriptions of the same module
  context and is accepted only from functions invoked from the frontend (resolvers), not from async events,
  scheduled runs or web triggers. Channel names are yours to choose; realtime tokens are optional (with tokens, an
  event reaches only subscriptions whose token carries the same claims). Global channels reach every user of the
  app, so payloads carry sprint ids only.

## 5. Sprint action (modal)

The sprint comes from the module context; the harness opens active and future sprints. A sprint that has not started
shows `[data-testid="not-started"]` (and optionally close) and nothing else. Otherwise:

- Team totals as in §4 (`[data-metric]`, same four), and `[data-testid="hidden-count"]` = number of this sprint's
  changes hidden from the viewer.
- `table[data-testid="ledger"]`, headers `th[data-col]` for `issue`, `points`, `kind`, `by`, `at`, `source`,
  `deployed`; one `tr[data-change-id="<changelog id>"]` per visible change with `td[data-col=…]` cells: issue key,
  current estimate, `added`/`removed`, author display name, a `<time datetime>` holding an ISO-8601 instant with
  offset (compared as instants), `event`/`reconcile`, and one `[data-env="<env>"]` element with the text
  `Deployed to <env>` per environment the issue was deployed to (§14; empty when none).
- Default order: `at` ascending, equal times by changelog id ascending (ids compare as numbers). Clicking
  `th[data-col="at"]` toggles between that order and its exact reverse, starting with ascending when another sort was
  active. The active header carries `aria-sort`.
- The issue key opens the issue (`/browse/<KEY>`) through the Forge router.
- Clicking a row selects it (`aria-selected="true"`). `[data-testid="post-summary"]` posts one comment on the selected
  change's issue, authored by the viewer, in Atlassian Document Format, naming the issue key, the sprint name and the
  sprint's creep; then a success flag. One click, or a double click, posts exactly one comment. A failure shows an
  error flag and leaves the modal working. With a `Comment group` set (§13), only members of that Jira group may post:
  anyone else gets the error flag and no comment is posted.
- `[data-testid="explain"]` asks Forge LLM (`@forge/llm`) to explain the sprint's creep, using any model that
  `list()` reports `active` (`chat()` or `stream()`). The model must answer through one tool, `report_scope`,
  arguments `{ "summary": string, "changeIds": string[] }` (force it with `tool_choice`); send it only what the viewer
  may see. `[data-testid="explanation"]` shows the summary only if it holds no digits (otherwise your own sentence
  with the ledger's numbers), plus one `[data-change-id]` element per returned id that is a visible change of this
  sprint (others dropped). A refusal (no tool call), malformed arguments or an LLM error shows an error flag and
  leaves the modal working. §16 adds to these rules.
- `[data-testid="close"]` closes the modal.

## 6. Rovo

`action` key `get-sprint-scope`, `actionVerb: GET`, one required string input `sprintId`. For the invoking person it
returns a JSON object:

```
{ "sprintId": "41", "sprintName": "…", "committed": 34, "added": 8, "removed": 3,
  "creepPercent": 23.5, "hiddenChanges": 1,
  "changes": [ { "changeId": "…", "issueKey": "OPS-12", "kind": "added", "points": 5,
                 "at": "<ISO-8601 UTC>", "by": "<display name>" } ] }
```

`creepPercent` is rounded as creep (§1) and `null` when committed is 0; `changeId` is the changelog id; `changes` are
the visible ones in table order (§5); `at` is compared as an instant. An unknown or missing `sprintId` returns
`{ "error": "<message>" }` and does not throw.

`skills/sprint-scope-analyst/SKILL.md`: YAML frontmatter `name` equal to the directory name (1–64 characters:
lowercase letters, digits, single hyphens, no leading or trailing hyphen), `description` (50–1,024 characters: what it
does and when to use it), `allowed-tools` (space separated, including `get-sprint-scope`); then at most 500 lines of
Markdown telling the agent when and how to call `get-sprint-scope`, what `sprintId` is, how to read the result, and
what to do with an error.

## 7. Custom UI

Every Custom UI surface calls `view.theme.enable()` and is styled with Atlassian design tokens (`var(--ds-…)`): text
with `--ds-text*` (links may use `--ds-link*`), contrast at least 4.5:1 in light and in dark (disabled controls
exempt). The page behind your surface is unpainted: paint your own background with a `--ds-surface*` token. The
browser console stays free of errors. Surfaces render inside the default Forge Custom UI content security policy:
inline `<script>` in `index.html` only as static content (the platform hashes it), no inline event handlers or `eval`,
no `<style>` elements or `style` attributes in markup, no external scripts, styles or fonts, assets referenced
relatively. Declaring `unsafe-inline` in `permissions.content.styles` is allowed; script relaxations count as unneeded
permissions. §17 budgets how the widget and the sprint action boot.

## 8. What the harness does differently from production

- Custom UI resources are served exactly as committed, uncompressed: build them yourself into
  `static/<name>/build/`. A UI Kit resource is bundled by the harness from its source file the way `forge deploy`
  does (the classic JSX transform: `import React` in every `.jsx` file). Backend source is bundled from `src/` the
  way `forge deploy` does.
- Every function invocation runs in a fresh Node process of the Forge runtime, on the virtual clock of §11.
- Trigger `filter.expression` is not evaluated: the handler receives every issue-updated event.
- A consumer that throws or is killed is redelivered after 1, 2, 4 and 8 virtual minutes, then every 15, for 24
  virtual hours; a retry request (`InvocationError`) is redelivered after its `retryAfter`.
- Jira requests made from Custom UI (`requestJira`) cost no points (as on Forge); they still count in their
  endpoint's burst bucket (§10).
- The scoring site uses a different seed than the dev site: ids, keys, custom field ids, users, groups, sprint names
  and dates all differ.
- The widget runs at the dashboard layout the harness chooses; the sprint action in a modal; the admin page in the
  harness's UI Kit host, which renders your `@forge/react` tree (no pixels are graded).

## 9. The v2 ledger and the live migration

v1 keeps its ledger in entity `scope-change` (see `manifest.yml`): one row per change, keyed `<changeId>:<sprintId>`,
holding `sprintId`, `changeId`, `at` (the change time, epoch milliseconds), `created` (the same instant, ISO-8601),
`issueId`, `issueKey`, `kind`, `authorId`, `authorName` and `source`. On the sites it holds the changes of each active
sprint's first 2 days.

v2 keeps its ledger in a NEW entity `scope-ledger`:
- Attributes (at least): `sprintId` string, `at` float, `changeId` string, `kind` string, `issueId` string,
  `issueKey` string, `estimate` float, `boardId` string, `estimateField` string, `deleted` boolean, `deployedEnvs`
  string; index `by-sprint`, partition `[sprintId]`, range `[at]`. A named index takes exactly ONE range attribute:
  Forge refuses more at lint and at deploy. Add attributes and indexes as you need; row keys are yours to choose.
- Per row: `at` the change time in epoch milliseconds; `kind` `added` or `removed`; `estimate` the issue's points
  when the row is written, in the estimation field the sprint's board uses then (no value = 0); `estimateField` that
  field's id; `boardId` that board's id; `deleted` true once the issue is deleted (§12); `deployedEnvs` the
  environments the issue was deployed to (§14), comma-separated, each once (empty when none). Once written, a row's
  `estimate`, `estimateField` and `boardId` never change.

Migration guarantees (graded):
- Every `scope-change` row appears in `scope-ledger` exactly once: the same change (changelog id + sprint) with its
  original `at`. Its kind, issue, author and `source` stay as v1 recorded them (§3, §5).
- `scope-change` stays declared exactly as in v1's manifest, and its rows are never changed or deleted.
- It runs while events keep flowing: a change v1 recorded that arrives again through events or reconciliation is still
  one row.
- It survives being cut off by time limits: a killed or retried invocation resumes where it stopped and leaves no
  duplicate.
- It is complete within the first 2 virtual hours after the upgrade, and surfaces show correct numbers while it runs.
- The admin panel's `Migration` text reads `Migrated <n> of <total> v1 rows` — `<total>` the number of v1 rows, `<n>`
  how many of them are in `scope-ledger` — and also contains `complete` once all are.

Storage answers with Forge's real errors: `FAIL_IF_EXISTS` on an existing key → 409 `KEY_CONFLICT`; a failed
transaction condition → 400 `CONDITIONAL_CHECK_FAILED`; a transaction of more than 25 operations → 422
`UNPROCESSABLE_ENTITY`; an undeclared entity → 404 `SCHEMA_NOT_FOUND`.

## 10. Points, bursts and dosing

`RATE-MODEL.json` is the rate model both sites enforce on every Jira request; its numbers are binding:
- **Quota:** 2,400 points per installation per virtual hour, reset at the top of each virtual hour, nothing carried
  over; a hard wall: once spent, every request is answered 429 until the reset. It is the app's fair share of its
  65,000-point Tier 1 pool, which every tenant of the app shares: in a busy hour the other tenants can bring the wall
  forward, so a quota 429 may arrive before your installation spent its 2,400 points.
- **Background** = product event triggers, async event consumers, scheduled triggers, the web trigger.
  **Person-facing** = resolvers invoked from a UI surface (Custom UI or UI Kit) and the Rovo action. Every Jira
  request counts, `asApp` and `asUser` alike.
- **Costs:** GET 1 · Jira Software GETs 1 · `GET`/`POST /rest/api/3/search/jql` 1 + 1 per 50 issues returned ·
  `POST /rest/api/3/changelog/bulkfetch` 2 per call (≤ 1,000 issues) · the app field-value API (§15) 1 + 1 per 50
  updates (≤ 200 updates per request) · any other POST, PUT or DELETE 2. Every started block of 50 counts.
- **Bursts:** each endpoint (method + path template) has a token bucket of 30 points, refilled at 5 points per virtual
  second. Per-issue writes: at most 1 write per issue per 2 virtual seconds (a comment, an edit and a field value are
  all writes to that issue).
- **Responses:** every response carries `X-RateLimit-Limit: 2400`; when fewer than 20 % of the hour's points (480)
  remain, also `X-RateLimit-Remaining` and `X-RateLimit-NearLimit: true`. A 429 has the body
  `{"errorMessages":["Rate limit exceeded"]}`, `Retry-After: <seconds>` and `RateLimit-Reason`, one of
  `jira-quota-tenant-based` (Retry-After = seconds to the next virtual hour), `jira-burst-based`,
  `jira-per-issue-on-write`.

Guarantees (graded; the site's own accounting per virtual hour and per invocation is what is measured):
- Background work never takes more than `Background share (%)` of any virtual hour's quota (default 70 % = 1,680
  points). Until fewer than 480 points remain no response shows the spend, so the app keeps its own count.
- No person-facing request is ever answered with a quota 429. The harness's own person-facing traffic in any hour stays
  below the share your background leaves free.
- The backfill and every reconciliation finish correctly inside that budget (§3).
- Retry-After is honoured, and the reason decides the reaction: `jira-quota-tenant-based` → no background request at
  all until the next virtual hour; `jira-burst-based` → slow that endpoint only (nothing to it before Retry-After);
  `jira-per-issue-on-write` → delay writes to that issue only. Each request sent before its Retry-After passed counts
  against you.

## 11. Virtual time and invocation limits

The harness runs on a virtual clock. Inside an invocation `Date.now()` and `new Date()` read it, and
`getAppContext().invocationRemainingTimeInMillis()` (`@forge/api`) reports the virtual time left. Each request a
function makes advances its invocation's clock: a GET 120 ms, a search page 300 ms, a `bulkfetch` 600 ms, a write
200 ms; a wait inside a function (`setTimeout`, sleep) counts at face value. Platform calls count the same way: a
storage read (`get`, `query`, a secret or entity get or query) 120 ms; a storage write (`set`, `delete`, a
transaction) 200 ms; a queue push, a Forge LLM call and a Realtime publish 200 ms each. CPU time is free.

| invocation | limit |
|---|---|
| UI resolver (Custom UI or UI Kit) | 25 s |
| async event consumer, scheduled trigger | 55 s by default, up to 900 s with `timeoutSeconds` |
| web trigger, Rovo action | 55 s |

Exceeding the limit kills the invocation: it returns no result, and what it already wrote stays written. A consumer
that returns an `InvocationError` (`retryAfter` ≤ 900 s) or is killed is redelivered, at least once and in any order.
Scheduled triggers run once per virtual hour and are not retried: a failed or killed run waits for the next. Product
event triggers are retried up to 4 times (after 1, 2, 4 and 8 virtual minutes, or the `retryAfter` they asked for).

Guarantees (graded):
- Work that cannot finish in one invocation continues in the next (a queue continuation, the next scheduled run).
- A Retry-After longer than the time left is never waited out inside the function: a consumer returns an
  `InvocationError` with `retryAfter`; a scheduled run stops and resumes from a later run.
- A killed invocation leaves no partial duplicate: whatever completes its work completes it exactly once.

## 12. A world that changes

While v2 runs, sprints close, issues move to another board's sprint, a board's estimation field changes, issues are
deleted, and people lose browse permission on a project. The dev site exercises every one of these at least once; the
scoring schedule is private. Guarantees (graded):
- **A sprint closes:** its ledger is final (no change after the close is recorded for it) and it leaves the widget,
  which shows active sprints only.
- **An issue moves to another board's sprint:** recorded as `removed` from the old sprint and `added` to the new one;
  the `added` row and the new sprint's numbers use the NEW board's estimation field.
- **A board's estimation field changes:** changes after the switch use the new field; earlier rows keep their
  `estimate`.
- **An issue is deleted:** its rows stay as history with `deleted: true`; it no longer counts in current scope (§1).
- **A person loses browse permission:** their next request shows none of the rows of the issues they can no longer
  browse (no stale cache).

## 13. The admin panel (UI Kit)

A `jira:adminPage` module with `render: native`, built with `@forge/react`. The harness finds its controls by these
visible labels, exactly:

| label | control |
|---|---|
| `Background share (%)` | number 10–90, default 70 (§10) |
| `AI explanations enabled` | toggle, default on (§16) |
| `Daily AI token budget` | number, default 200000 (§16) |
| `Comment group` | text: a Jira group name; empty = everyone who can browse the issue may post the summary (§5) |
| `Save settings` | button: saves the four settings above |
| `Rotate CI secret` | button: makes a new CI secret (§14) and shows it once |
| `Migration` | read-only text (§9) |
| `Recent admin changes` | table, columns `when`, `who`, `what`: the last 20 admin changes, newest first |

- `Save settings` stores the four values together. A value outside its range (share not a whole number 10–90, budget
  not a whole number ≥ 0) is refused: nothing is saved and the panel shows an error. A Save that changes nothing adds
  no row.
- After `Rotate CI secret` the panel shows `CI secret: <secret>` once; from then on (later renders, reloads, every
  resolver answer) only `CI secret: ••••<last4>`. A secret is at least 32 characters, with no spaces.
- `Recent admin changes`: one row per Save that changed something and per rotation; `when` an ISO-8601 instant, `who`
  the acting person (account id or display name), `what` names each changed setting by its label (a rotation reads
  `Rotate CI secret`) and never holds a secret.

**Authorization (graded).** Any user who can load a surface can invoke every resolver attached to it, directly and
with any payload. So every resolver the admin page uses, reads included, checks on the server that the caller holds
Jira's global `ADMINISTER` permission: `GET /rest/api/3/mypermissions?permissions=ADMINISTER` as the user. The caller's
identity always comes from the resolver's `context` (`context.accountId`), never from the payload. A caller without
ADMINISTER gets `{ "error": "…" }` and nothing changes: no setting, no secret, no audit row, nothing disclosed.

## 14. CI deployment events (web trigger)

A `webtrigger` module, static: `response.type: static` with its outputs (`key`, `statusCode`, `contentType`, `body`)
declared in the manifest, and your function returns `{ "outputKey": "<key>" }`. CI posts deployment events in Forge's
documented web-trigger request shape: `body` is the raw string; `headers` maps each header name to an array of
strings, and header names may arrive in any letter case.
- Body: JSON `{"eventId": string, "sentAt": unix seconds, "environment": "staging" | "production", "issueKeys": [string, …]}`.
- Headers: `X-LZ-Timestamp: <unix seconds>` and
  `X-LZ-Signature: sha256=<hex HMAC-SHA256(secret, "<timestamp>.<raw body>")>`: `<timestamp>` is that header's value,
  `<raw body>` the body string exactly as received, the key the secret's UTF-8 bytes.
- Checks, in this order: the signature (compared with `crypto.timingSafeEqual`), then the timestamp (reject when
  |now − timestamp| > 300 s), then the `eventId`. Answers and effects (graded):

| request | answer | effect |
|---|---|---|
| no signature, a wrong one, a body changed after signing, or a timestamp more than 300 s from now | `401` | none |
| valid, with an `eventId` already accepted (also when both arrive at once) | `200` | none |
| valid, new `eventId` | `202` | the deployment is recorded |

Recorded: every `scope-ledger` row of each referenced issue carries the environment in `deployedEnvs` (rows written
later for that issue carry it too), and the sprint action's `deployed` cell (§5) shows `Deployed to <env>`. An
`eventId` is applied at most once, ever. Before any rotation there is no secret and every request gets `401`. The
secret comes only from the admin panel's `Rotate CI secret`, is stored only with `kvs.setSecret` (never with
`kvs.set`, in an entity, a log line, or any resolver answer but that rotation's), and no resolver returns it except as
`••••<last4>`.

## 15. The `scope-status` custom field

A `jira:customField` module, key `scope-status`, `type: string`, `readOnly: true`. The app writes its values as the
app (`asApp`), in bulk, through the app field-value API (`POST /rest/api/3/app/field/value` or
`PUT /rest/api/3/app/field/{fieldIdOrKey}/value`; at most 200 updates per request). Its field id differs per site:
find it with `GET /rest/api/3/field`, where it is the custom field whose `name` is your module's `name`.

Value per issue (graded), from the facts of §1:
- in an active sprint S now: `committed` if it was in S at S's `startDate`, else `added +<points>` with its current
  estimate for S written as §1 points (`added +5`, `added +2.5`, `added +0`);
- in no active sprint now, but in a currently active sprint S at some time after S's `startDate`: `removed`;
- any other issue: empty (no value), including the issues of a sprint once it closes.

Fresh within the same virtual hour as the change: the harness compares every issue's value with the site's state at
the end of each scored hour.

## 16. Forge LLM, properly

§5's explanation rules stay: numbers only from your ledger, nothing hidden from the viewer in any prompt,
`report_scope` forced. In addition (graded):
- **Model output is untrusted.** Issue text can carry instructions, so a returned tool call may be manipulated. Before
  acting on any tool call, validate it against the viewer's sprint scope: only `report_scope` is accepted, and only
  its `changeIds` that are visible changes of this sprint for this viewer are shown. No tool call makes the app do
  anything but show that validated explanation.
- **429:** Forge LLM answers 429 without a Retry-After. Back off between attempts (never retry at once), at most 3
  attempts per virtual minute, then show the error flag.
- **Incomplete answers:** a response or stream whose final choice has no `finish_reason` is a failure: show the error
  flag, never its content.
- **Cache:** an identical explanation request (same sprint, same content sent to the model for what this viewer may
  see) within 10 virtual minutes of a successful answer is served from cache, with no new Forge LLM call. Nobody is
  served an explanation built from changes they cannot see.
- **Admin controls, enforced server-side:** with `AI explanations enabled` off, or once the virtual day's tokens (the
  `usage.total_tokens` each Forge LLM response reports) reach `Daily AI token budget`, the explain resolver makes no
  Forge LLM call, even when invoked directly, and the modal shows the error flag. A virtual day is a UTC calendar
  day of the virtual clock.

## 17. Boot budget (Custom UI)

Counted, never timed (graded):
- The widget view and the sprint action each make at most 1 `invoke` before their first data paint, load at most 150
  KB (153,600 bytes) of JavaScript and CSS before it, and never request another origin. First data paint: the first moment the
  surface shows its `[data-metric]` numbers, or `[data-testid="needs-config"]` (widget) or
  `[data-testid="not-started"]` (sprint action). Bytes: your own `.js` and `.css` files as served, uncompressed; the
  platform's injected scripts and the Atlassian design-token stylesheets do not count.
- The admin page makes at most 1 `invoke` before its first render that shows the stored settings.

## 18. v1 keeps working

Everything in §1–§8 holds throughout the 6 scored hours: during the migration, under the quota, through the world
changes. The harness reads every surface at several points of the scored window, from the end of the first scheduled
run on, and compares it with the site's state at that point.
