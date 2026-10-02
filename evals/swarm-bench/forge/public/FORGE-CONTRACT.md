# Scope Ledger — Forge app contract (forge-1.0)

Agile coaches on a Jira Cloud site want to know what entered a sprint after it started, who added
it and how many story points it carried — on a dashboard, from the sprint itself, and from Rovo.
Build the Atlassian Forge app in this workspace. The harness installs it into a seeded Jira site
(scrum boards, parallel active sprints, a few hundred issues, people with different permissions)
and grades it by running it: product events, queue deliveries, scheduled runs, resolver calls,
the Rovo action, and every Custom UI surface in a browser, light and dark. Nothing is deployed.

## 1. The numbers

For each **active** sprint S, with `startDate` from the Jira Software sprint API:

- A **change** is a Sprint-field changelog entry created after `startDate` that puts an issue
  into S (`added`) or takes it out of S (`removed`). Its id is the changelog id, its time the
  changelog `created`, its author the changelog author. A change is keyed by changelog id +
  sprint: one entry moving an issue between two sprints is a `removed` in one and an `added` in
  the other.
- **committed** = sum of current estimates of issues that were in S at `startDate`.
- **added** = sum of current estimates of issues in S now that were not in S at `startDate`.
- **removed** = sum of current estimates of issues in S at some time after `startDate` and not in
  S now.
- **creep** = `100 × added / committed`, rounded half away from zero to one decimal and always
  written with that decimal and `%` (`20.0%`); `—` when committed is 0.
- An issue's **estimate** for S is the value of the estimation field of S's board (the sprint's
  `originBoardId`); no value counts as 0. "After `startDate`" is strictly after.
- Points are written as plain decimals (`34.5`, `0`), no thousands separators.

Background work (triggers, consumer, scheduled job) sees every issue. What a **person** sees in
the sprint action and the Rovo action lists only changes to issues that person can browse, plus
a count of the changes hidden from them; the totals above are team totals and are the same for
everyone.

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
| `llm` | Forge LLM (`model: [claude]`), for the sprint action's explanation (§5) |

Resolvers use `@forge/resolver`. Storage is Forge KVS: ledger changes live in a **custom entity
indexed by sprint (partition) and change time (range)**, and are read back through that index.
Request only the scopes your calls need: per call, the OAuth2 scopes the shipped OpenAPI lists for
it (the classic scope where one exists, else the whole granular set). The linter does not see every
call. Custom UI only; UI Kit (`render: native`) earns nothing.

## 3. Backend behaviour

- The site existed before the app was installed. The scheduled job's first run backfills every
  change since each active sprint started, including issues that have since left every sprint.
- Issue updates keep the ledger current: sprint changes add ledger rows; estimate changes move
  the numbers. Updates that touch neither do no Jira or queue work (KVS reads are fine). The
  harness runs the scheduled job once before it delivers any update.
- Product events can arrive more than once and out of order, and some never arrive. The ledger
  holds **exactly one row per change**, whatever the delivery history. Each row records `source`:
  `event` or `reconcile`, the path that recorded it first (event work may also record other
  changes of the issue it reads). Later scheduled runs record what the event stream missed; a run
  with nothing new writes nothing.
- Jira may answer `429` with `Retry-After`: wait at least that long before the next attempt.
- Background work uses `asApp()`. Whatever shows a person issue data (keys, authors, change
  lists) shows only what that person can browse — read as them (`asUser()`), or as the app with
  an explicit permission check for them. Comments are posted as the person. Team totals come
  from the ledger.

## 4. Dashboard widget

**Edit** (`edit.resource`): one element per scrum board, `[data-testid="board-option"]` with
`data-board-id`, clickable; the selected one carries `aria-pressed="true"`. The choice reaches the
dashboard through the dashboards widget edit API (`@forge/dashboards-bridge`). The dashboard's
own Save calls your `onProductSave` handler with the last `updateConfig` value (the stored config
if none was sent) and stores what it returns (`null` stores nothing); with no handler registered
it stores the last `updateConfig` value. Reopening edit shows the
stored board selected.

**View** (`resource`), root `[data-testid="scope-widget"]`. It takes its board from the widget
configuration in its context, so two widgets on one dashboard can show different boards:
- No stored board: `[data-testid="needs-config"]`, nothing else.
- Otherwise one `[data-testid="sprint"][data-sprint-id="<id>"]` per active sprint of that board,
  ordered by `startDate` (ties by sprint id), each holding `[data-metric="committed"]`, `[data-metric="added"]`,
  `[data-metric="removed"]` and `[data-metric="creep"]` whose text is the §1 number.
- A bar chart: one `<svg data-testid="chart">`; per sprint and per series one
  `<rect data-sprint-id data-series="committed|added|removed">`; every bar on one linear scale
  from 0 (rendered height proportional to its number within 1 px).
- At 380 px wide nothing scrolls horizontally and no number is clipped or truncated (long
  names may end in an ellipsis).
- Live: after ledger rows are written, an open widget shows the new numbers without a reload,
  through Forge Realtime (`@forge/realtime` in the backend, the bridge's realtime subscribe in the
  widget) — no polling. Realtime payloads carry sprint ids only.

## 5. Sprint action (modal)

The sprint comes from the module context; the harness opens active and future sprints. A sprint
that has not started shows `[data-testid="not-started"]` (and optionally close) and nothing else.
Otherwise:

- Team totals as in §4 (`[data-metric]`, same four), and `[data-testid="hidden-count"]` = number
  of this sprint's changes hidden from the viewer.
- `table[data-testid="ledger"]`, headers `th[data-col]` for `issue`, `points`, `kind`, `by`,
  `at`, `source`; one `tr[data-change-id="<changelog id>"]` per visible change with
  `td[data-col=…]` cells: issue key, current estimate, `added`/`removed`, author display name, a
  `<time datetime>` holding an ISO-8601 instant with offset (compared as instants),
  `event`/`reconcile`.
- Default order: `at` ascending, equal times by changelog id ascending (ids compare as numbers).
  Clicking `th[data-col="at"]` toggles between that order and its exact reverse, starting with
  ascending when another sort was active. The active header carries `aria-sort`.
- The issue key opens the issue (`/browse/<KEY>`) through the Forge router.
- Clicking a row selects it (`aria-selected="true"`). `[data-testid="post-summary"]` posts one
  comment on the selected change's issue, authored by the viewer, in Atlassian Document Format,
  naming the issue key, the sprint name and the sprint's creep; then a success flag. One click,
  or a double click, posts exactly one comment. A failure shows an error flag and leaves the
  modal working.
- `[data-testid="explain"]` asks Forge LLM (`@forge/llm`) to explain the sprint's creep, using a
  model that `list()` reports `active`. The model must answer through one tool, `report_scope`,
  arguments `{ "summary": string, "changeIds": string[] }` (force it with `tool_choice`); send it
  only what the viewer may see. `[data-testid="explanation"]` shows the summary only if it holds
  no digits (otherwise your own sentence with the ledger's numbers), plus one `[data-change-id]`
  element per returned id that is a visible change of this sprint (others dropped). A refusal (no
  tool call), malformed arguments or an LLM error shows an error flag and leaves the modal working.
- `[data-testid="close"]` closes the modal.

## 6. Rovo

`action` key `get-sprint-scope`, `actionVerb: GET`, one required string input `sprintId`. For
the invoking person it returns a JSON object:

```
{ "sprintId": "41", "sprintName": "…", "committed": 34, "added": 8, "removed": 3,
  "creepPercent": 23.5, "hiddenChanges": 1,
  "changes": [ { "changeId": "…", "issueKey": "OPS-12", "kind": "added", "points": 5,
                 "at": "<ISO-8601 UTC>", "by": "<display name>" } ] }
```

`creepPercent` is rounded as creep (§1) and `null` when committed is 0; `changeId` is the
changelog id; `changes` are the visible ones in table order (§5); `at` is compared as an instant.
An unknown or missing `sprintId` returns `{ "error": "<message>" }` and does not throw.

`skills/sprint-scope-analyst/SKILL.md`: YAML frontmatter `name` equal to the directory name
(1–64 characters: lowercase letters, digits, single hyphens, no leading or trailing hyphen),
`description` (50–1,024 characters: what it does and when to use it), `allowed-tools` (space
separated, including `get-sprint-scope`); then at most 500 lines of Markdown telling the agent
when and how to call `get-sprint-scope`, what `sprintId` is, how to read the result, and what to do
with an error.

## 7. Custom UI

Every surface calls `view.theme.enable()` and is styled with Atlassian design tokens
(`var(--ds-…)`): text with `--ds-text*` (links may use `--ds-link*`), contrast at least 4.5:1 in
light and in dark (disabled controls exempt). The page behind your surface is unpainted: paint your own background with a
`--ds-surface*` token. The browser console stays free of errors. Surfaces render inside the
default Forge Custom UI content security policy: inline `<script>` in `index.html` only as static content (the
platform hashes it), no inline event handlers or `eval`, no `<style>` elements or `style` attributes in markup, no external scripts, styles or fonts, assets referenced relatively.
Declaring `unsafe-inline` in `permissions.content.styles` is allowed; script relaxations count as
unneeded permissions.

## 8. What the harness does differently from production

- Resources are served exactly as committed (build Custom UI yourself). Backend source is bundled
  from `src/` the way `forge deploy` does. Every function invocation runs in a fresh Node process
  of the Forge runtime, with the platform's timeouts.
- Trigger `filter.expression` is not evaluated: the handler receives every issue-updated event.
- A consumer that throws or times out is redelivered after 1, 2, 4 and 8 minutes, then every 15,
  for 24 hours; a retry request (`InvocationError`) is redelivered after its `retryAfter`. The
  harness clock runs with real time and jumps over these redelivery waits; a wait inside an
  invocation is real time.
- The scoring site uses a different seed than the dev site: ids, keys, custom field ids, users,
  sprint names and dates all differ.
- The widget runs at the dashboard layout the harness chooses; the sprint action in a modal.
