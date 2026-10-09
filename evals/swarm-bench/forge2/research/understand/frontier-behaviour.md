# Frontier behaviour on Forge 1.0: how the strongest models actually worked, and where no headroom is left

Read-only study, 2026-10-09. Nothing was run except sqlite3/python reads of finished artefacts (no scorer, no
browser, no build). A paid run was building on the machine; load was checked before starting.

## Sources and method

- Haiku 5.5 (0.9661): tree `~/Library/Application Support/Goose/benchmark/runs/build/openrouter-cloud-fac49b17-8668-42f2-91ea-78679cee321c-r0`,
  session `.openrouter-cloud-fac49b17-…-r0-runtime/goose/data/sessions/sessions.db` (283 messages, 99 tool calls),
  `telemetry.jsonl` (72 entrant calls + 1 gemini-2.5-flash title call), `verdict.json`, `forge-observations.json`,
  the final `src/*.js` and `manifest.yml`.
- Pareto 26.10 Preview (0.9450): same layout, id `a221226b-f2db-4878-8e82-d905cf5df57a` (126 messages, 50 tool calls,
  50 entrant calls).
- Board rows for all 31 entries: `scratchpad/forge-review/forge-board.json` (`result[].checksSummary[]`, 66 rows each).
- Score composition from `evals/swarm-bench/bench/score_forge.py` (TIER_WEIGHT, `E_WEIGHT = 0.12`, the three E rows,
  `r_as_app` / `r_completes_in_timeout` preconditions); the pack model from `forge_oracle.py`; the golden's event path
  from `golden-forge/src/sync.js`; Forge limits from the docs pages in `scratchpad/forge2/pages/`.
- Working files (full and compact transcript dumps, per-call tables, the scripts that made them):
  `scratchpad/frontier-work/` (`haiku.txt`, `pareto.txt`, `*-compact.txt`, `haiku-calls.txt`, `pareto-calls.txt`).

A "call" is one entrant-model request in telemetry. Assistant rows were assigned to calls by timestamp (every row
assigned, none left over) and each call was classified by what its tool calls did. "Elapsed" is telemetry `total_ms`;
calls tile the wall clock with zero gap, so it includes the tool time the call waited on.

Caveats. Pareto's reasoning is not exposed (no thinking blocks; OpenRouter reported 0 reasoning tokens), so its own
words are seven short status messages plus its code comments. GPT-6.1 Sol, Opus 5.5 and Sol Pro trees are not on this
machine: for them the evidence is the board rows and wall times only, and anything said about how they work is
inference from Haiku and Pareto, which sit 0.0108 and 0.0319 below them.

## 0. Bottom line

1. On everything Forge 1.0 states, the frontier has no headroom. Sol, Opus 5.5, Sol Pro and Haiku score 1.0 on all
   63 non-excellence rows, on all three scoring seeds. Their entire residual (0.0231 to 0.0339) is `e_event_economy`
   (3.3x to 3.9x the one-read-per-change optimum). The contract never states that target; the task prompt only says "A
   small excellence share rewards few Jira requests". For Sol Pro and Haiku there is also the one-call cliff of
   `e_reconcile_economy` (18 and 16 calls against 15). Pareto's extra 0.0308 is a scorer precondition that zeroes two rows because its scheduled
   trigger hands the backfill to a 900-second queue consumer, which is the more scale-robust design.
2. The contract did the hard thinking. Every semantic trap that both models handled traces to a contract sentence
   that Haiku paraphrased in its own design notes. Testing found no semantic bug in either app. All six defect
   classes Haiku found by testing were platform wiring, each announced by an explicit dev-tool error string. Haiku
   never exercised the restricted viewer, the LLM failure branches, the comment refusal, sorting or the router, and
   it scored 1.0 on all of them.
3. The budget never bound. They used 72 and 50 of 150 calls, both `stopped_by: model_finished`, in 20 and 10 minutes.
4. The scoring world is small and static. It has 2 boards, 3 active sprints, 55 to 72 historical changes and about
   80 to 104 ledger rows. The live phase delivers about 41 issue updates one at a time (measured overlap: 0), with
   duplicates, permutations and drops, and the longest Retry-After is 30 seconds. Sprints, boards and estimation
   fields never change, and no issue is ever deleted (the pack model makes all of these static).
5. So what would plausibly defeat these models is not more of Forge 1.0. It is concurrency with a throughput
   requirement, scale against real Forge limits, idempotency under ambiguous failures and a Retry-After longer than
   the invocation, a world that changes mid-run, a live migration of installed data, and a contract that states
   invariants instead of listing the faults it will inject. Pure volume, API trivia, tighter call budgets and
   unstated grader preferences would not.

---

## 1. (a) Session timelines

### 1.1 Haiku 5.5: 72 calls, 17:18:27 to 17:38:31 (20.1 min), 0.9661

| phase | clock | calls | what happened |
|---|---|---|---|
| 1 | 17:18:30-17:18:35 | C1-C2 (2) | `cat FORGE-CONTRACT.md STARTER.md BROWSER-TESTING.md`, then manifest, package, benchmark-prompt, workspace files |
| 2 | 17:18:40-17:19:12 | C3-C11 (9) | kit survey: forge-dev help, lint.cjs, KIT.json. Five attempts to navigate the manifest schema (C5-C9: "The schema isn't shaped the way I guessed, so I'll search it for the module definitions directly."). Emulator/proxy routes |
| 3 | 17:20:42-17:23:29 | C12-C21 (10) | typings (kvs, realtime, llm, dashboards-bridge, bridge, resolver, events), OpenAPI ops and scopes, Rovo/consumer schemas, emulator context. Three long design calls: C12 90 s/15.3k tokens, C17 61 s/10.6k, C18 79 s/15.1k |
| 4 | 17:24:34 | C22 (1) | `forge-dev users`, then a direct `curl` to the dev site with the URL's credentials ("Client must be authenticated to access this resource."). 65 s/11.8k tokens |
| 5 | 17:26:17 | C23 (1) | storage/index schema, `@forge/api` router typings; 103 s/20.3k tokens |
| 6 | 17:30:55 | C24 (1) | wrote manifest.yml, src/jira.js, src/ledger.js, src/index.js and src/llm.js in a single 278 s response (65,531 output tokens) |
| 7 | 17:31:27 | C25 (1) | self-review: 5 edits (one of them wrong, see section 2), then first lint: 3 manifest errors |
| 8 | 17:31:34-17:31:57 | C26-C30 (5) | lint loop: `storage` belongs under `app`, `functions` is not a root key, widget `thumbnail`, attribute type `float`, missing resources and skill dir |
| 9 | 17:32:08-17:33:48 | C31-C36 (6) | SKILL.md, widget, widget-edit, sprint-modal, CSS, thumbnail PNG, build script. A sed failure. Build and lint clean |
| 10 | 17:34:24 | C37 (1) | self-review of the event path (switches to `fields=*all`), reset, scheduled run (fails: no `timeout` on macOS) |
| 11 | 17:34:27-17:34:43 | C38-C41 (4) | backfill: 401 "Unauthorized; scope does not match" on `GET /rest/agile/1.0/board`. Adds `read:project:jira`; backfill OK, 75 rows |
| 12 | 17:34:53-17:35:22 | C42-C46 (5) | events: consumer wired as `resolver.method` gives "no function 'undefined' in the manifest", 30 redeliveries, DROPPED. Fixed. Duplicate delivery is a no-op; second scheduled run confirmed as a no-op |
| 13 | 17:35:25-17:35:59 | C47-C55 (9) | widget/modal resolvers: resolver keys missing on modules (3 calls); KVS page limit 200 > 100 (4 calls, two wrong hypotheses first); numbers checked by hand |
| 14 | 17:36:02-17:36:31 | C56-C61 (6) | Rovo sort order; LLM explain (clean answer only); comment 400 "Comment body is not valid!", then body fix, then 201 twice; cross-sprint move check; serve widget |
| 15 | 17:36:49-17:37:53 | C62-C67 (6) | browser: widget light, edit and modal screenshots (3 images read), dark mode and the 380 px overflow measurement |
| 16 | 17:38:03-17:38:24 | C68-C71 (4) | realtime: the first publishes had "no matching subscription", so it delivered sprint-changing events while the widget was open: "delivered to 2 subscription(s)". Stop servers, lint, build |
| 17 | 17:38:31 | C72 (1) | final summary |

By activity:

| activity | calls | elapsed s | output tokens |
|---|---|---|---|
| read contract + workspace | 2 | 8 | 603 |
| explore kit/schema/typings/OpenAPI (offline) | 20 | 397 | 68,231 |
| probe dev site before writing (users + curl) | 1 | 65 | 11,780 |
| write code | 5 | 367 | 86,601 |
| self-review fixes (no test involved) | 2 | 67 | 11,855 |
| lint / manifest-schema repair loop | 7 | 53 | 6,282 |
| failing forge-dev run, diagnosis and fix | 16 | 84 | 8,672 |
| forge-dev / browser verification that passed | 18 | 155 | 11,326 |
| final summary | 1 | 8 | 1,341 |

Shape. Haiku spent 23 calls and 12.5 minutes (62% of its wall clock) before the first file existed, and most of that
was long reasoning. It then wrote the whole backend in one response. Of the 48 calls after the first write, 25 went
into wiring defects (7 lint/manifest repair, 16 failing dev runs, 2 self-review), 18 into verification that passed, 4
into more writing and 1 into the summary.

### 1.2 Pareto 26.10 Preview: 50 calls, 09:48:44 to 09:59:02 (10.3 min), 0.9450

| phase | clock | calls | what happened |
|---|---|---|---|
| 1 | 09:48:46-09:48:51 | C1-C2 (2) | `ls`/`tree`, then one `cat` of FORGE-CONTRACT, STARTER, BROWSER-TESTING, package.json, manifest, browser-self-test |
| 2 | 09:48:52-09:48:59 | C3-C7 (5) | kit dir; `forge-dev users` + `kvs` (dev site, empty); help + KIT.json; `@forge/*` typing dirs; kvs entity typings |
| 3 | 09:49:04-09:49:37 | C8-C13 (6) | manifest-schema walk for the module shapes (one EPERM writing /tmp), sprintAction + entity schema, OpenAPI ops/scopes/bulk-changelog beans, AppSchema, kvs/queue/realtime typings |
| 4 | 09:49:58 | C14 (1) | manifest.yml |
| 5 | 09:50:09 | C15 (1) | bridge, kvs query, flag and resolver typings |
| 6 | 09:50:30-09:51:51 | C16-C19 (4) | src/jira.js, src/ledger.js, src/index.js, SKILL.md (one file per call) |
| 7 | 09:52:00 | C20 (1) | placeholder build dirs + `npm run lint`: "No issues found." on the first run |
| 8 | 09:52:03-09:52:30 | C21-C25 (5) | backfill (log dir missing once); KVS inspection; dashboards-bridge source; sprint resolver; second run: "Writes on second run: 0" |
| 9 | 09:52:40-09:54:14 | C26-C32 (7) | ui/common, widget, edit, sprint, styles.css, build.mjs; build + lint + serve three surfaces |
| 10 | 09:54:22-09:54:33 | C33-C35 (3) | test-browser.mjs, run, widget screenshot |
| 11 | 09:54:46-09:55:39 | C36-C38 (3) | `events --limit 12` + KVS diff; dark serves + `forge-dev llm` (the LLM script) + widget resolver; committed sums + `events --limit 100` |
| 12 | 09:55:58-09:56:23 | C39-C41 (3) | test-interactions.mjs: dark, 380 px, saved board reopened, sort toggle exact reverse, comment refusal on AUTH-298, double click, all five scripted LLM answers |
| 13 | 09:56:41-09:56:54 | C42-C43 (2) | test-live.mjs: open widget, run the heal, DOM changes without reload and equals a fresh resolver result, next run makes no writes or publishes |
| 14 | 09:57:12-09:57:47 | C44-C45 (2) | Rovo action: valid, missing and unknown sprint; second viewer |
| 15 | 09:58:17-09:58:25 | C46-C47 (2) | test-ledger.mjs (in-memory KVS unit test) + lint |
| 16 | 09:58:45-09:58:54 | C48-C49 (2) | README, `npm test` script, stop servers and check they stopped |
| 17 | 09:59:02 | C50 (1) | final summary |

By activity:

| activity | calls | elapsed s | output tokens |
|---|---|---|---|
| read contract + workspace | 2 | 6 | 170 |
| explore kit/schema/typings/OpenAPI (offline) | 11 | 55 | 2,560 |
| probe dev site before writing (users + kvs) | 1 | 2 | 59 |
| write code | 11 | 217 | 15,775 |
| placeholder build + lint (clean first time) | 1 | 9 | 157 |
| forge-dev backfill/events/resolver checks (all passed) | 9 | 137 | 3,681 |
| UI build, serve and browser tests (all passed) | 10 | 117 | 4,814 |
| ledger unit test | 2 | 38 | 1,874 |
| README + cleanup | 2 | 29 | 1,256 |
| final summary | 1 | 8 | 739 |

Shape. Pareto wrote its first file 74 seconds in, one file per call. It was lint-clean on the first run, and its first
backfill was right first time ("The dev run confirms that the ledger is populated: KVS contains 55 changes and 81
issue/sprint records…"). It had zero failing dev runs apart from two shell trivia (a sandboxed /tmp and a missing log
directory). It spent 21 of its 50 calls verifying, mostly through test scripts it wrote itself. Its opening statement
is the plan it then carried out: "I've read the contract. I'm building the shared ledger first, then wiring the
widget, sprint modal, and Rovo to the same permission-filtered read path. I'll verify backfill and event handling
against the dev site before testing the UI."

### 1.3 Side by side

| | Haiku | Pareto |
|---|---|---|
| calls / minutes before the first file | 23 / 12.5 | 13 / 1.2 |
| files per write call | 5 in one call, then 1 to 3 | 1 |
| lint runs that had errors | 3 (8 errors, 1 warning; repaired over 5 calls) | 0 |
| failing dev runs (non-trivial) | 6 defect classes | 0 |
| looked at dev-site DATA before writing | no (users list; curl refused) | no (users list; empty kvs) |
| own test scripts | none (ad-hoc invokes, screenshots) | 4 |
| output tokens | 206,691 (150,848 reasoning) | 31,085 |

Neither model explored the dev site's data or event shapes before writing code. Both coded from the contract, the
typings, the manifest schema and the offline OpenAPI, then used the dev site as a validator.

---

## 2. (b) Contract traps: handled from the text, or discovered by a failing test

Legend. TEXT means the trap was in the first implementation and traces to contract wording. +T means it was also
exercised on the dev site and passed first time. FAIL->FIX means a failing dev run surfaced it and the model then
fixed it. NOT RUN means the model never exercised it. Board results are 1.0 for both models on every row below.

| # | trap (source) | Haiku | Pareto |
|---|---|---|---|
| 1 | change key = changelog id + sprint; a move between two sprints is a `removed` plus an `added` (§1) | TEXT+T: "keying each change by changelog ID plus sprint ID so moves between sprints register as both a removal and an addition"; later "changelog 76926 is a `removed` in sprint 952 and an `added` in sprint 1194, one row each" | TEXT+T (unit test "cross-sprint move") |
| 2 | "after startDate" is strict (§1) | TEXT ("verifying that boundary changes exactly at startDate are properly excluded"); NOT RUN | TEXT+T (unit test "start boundary") |
| 3 | estimate from the sprint's `originBoardId` board; none = 0 (§1) | TEXT ("a sprint's originBoardId may differ from the board I'm iterating over") | TEXT+T (unit test "different board estimates") |
| 4 | creep half away from zero, one decimal, `—` at committed 0 (§1) | TEXT ("avoids floating-point precision issues near .05 boundaries"); checked two values by hand | TEXT+T (unit test "zero commitment") |
| 5 | first run backfills issues that left every sprint (§3) | TEXT ("The tricky part is finding issues that left a sprint — since JQL doesn't support a "sprint was" operator…") | TEXT (code comment: "Unlike sprint=current, updated finds issues which left every sprint before installation.") |
| 6 | updates touching neither field do no Jira or queue work (§3) | TEXT+T ("Those two events were priority and label changes, so they correctly publish nothing.") | TEXT+T ("unrelated updates do no Jira or queue work") |
| 7 | the harness runs the scheduled job once before any update (§3, a guarantee rather than a trap) | relied on it ("I'm relying on a KVS-cached config written by the scheduled job that runs before any updates are delivered, so that lookup is always safe and available.") | relied on it (the consumer returns `{changedSprints: []}` when no config exists) |
| 8 | duplicates, reordering, loss give exactly one row; `source` is the first path (§3) | TEXT+T (FAIL_IF_EXISTS; "the same changelog 76833 was delivered twice and the second delivery is a no-op") | TEXT+T (`source: previous?.source \|\| source`; unit test "duplicates, source provenance") |
| 9 | a run with nothing new writes nothing (§3) | TEXT+T (briefly misread 77 sets as a failure, then confirmed 0) | TEXT+T ("Writes on second run: 0", no publishes) |
| 10 | 429: wait at least Retry-After (§3) | TEXT; NOT RUN (no 429 in its visible output); parses numeric seconds only | TEXT+T (dev backfill hit `GET /rest/api/3/field -> 429 Retry-After 2`; code: "respecting both forms of Retry-After") |
| 11 | asApp for background work; a person sees only browsable changes plus a hidden count (§3, §5, §6) | TEXT; restricted viewer NOT RUN (its own words, see section 3 below) | TEXT+T ("Two viewers received different visible-change counts and identical team totals") |
| 12 | scopes exactly as the OpenAPI lists per call (§2) | FAIL->FIX: 401 "scope does not match" on the board list. It had printed that operation's scopes `["read:board-scope:jira-software","read:project:jira"]` in C17 and still omitted `read:project:jira` | TEXT (lint clean; identical final 7 scopes) |
| 13 | manifest wiring: entity storage, widget thumbnail, consumer `function`, resolver on modules (§2) | FAIL->FIX: 3 failing lint runs repaired over 5 calls; consumer redelivery storm ("no function 'undefined' in the manifest… DROPPED: retention window exceeded"); "function 'resolver' is referenced by no module" | TEXT (lint clean on the first run) |
| 14 | KVS entity query page limit 100 (platform limit, not in the contract) | FAIL->FIX: "The page limit must be set between 1 and 100." It guessed Jira twice ("The Agile sprint list accepts maxResults only up to 100", "The error comes from my bulk changelog request") before grepping the emulator: "That message is a KVS error, not a Jira one." | used `limit(100)` from the start |
| 15 | comment: ADF as the viewer, one per click or double click, error flag on failure (§5) | body shape FAIL->FIX (its self-review broke it, see below). Double click NOT RUN (UI in-flight flag: "prevent duplicate comments from double-clicks by disabling the UI button while the request is in flight rather than adding backend-side deduplication"). Refusal NOT RUN | TEXT+T: chose AUTH-298 (the issue STARTER says refuses comments) — "Post request count (one refusal, one double-click) 2" |
| 16 | LLM: forced `report_scope`, digits rule, unknown ids dropped, refusal/malformed/error give an error flag (§5) | TEXT; only the clean answer was run | TEXT+T: found the script with `forge-dev llm` ("script: clean -> digits -> refusal -> malformed -> error") and tested all five answers |
| 17 | realtime: global channel, sprint ids only, no polling, live numbers (§4) | TEXT; first check showed "no matching subscription" (the publishes predated the widget), then "delivered to 2 subscription(s)" | TEXT+T: "the already-open widget changed from 24.1% to 39.8% without a reload" |
| 18 | widget edit: board options, `aria-pressed`, onProductSave, reopen shows the stored board (§4) | TEXT (relies on the host storing the last `updateConfig`; no handler); edit surface only screenshot-checked | TEXT+T (onProductSave; "Saved selection reopened successfully") |
| 19 | sort toggle and `aria-sort`, router, selection, close, not-started (§5) | NOT RUN ("The sprint-action sort toggle, row selection, router links and the explain button are coded but not exercised in a browser.") | sort toggle +T; others TEXT |
| 20 | tokens, dark mode, 380 px, CSP, clean console (§4, §7) | TEXT+T (caught its own inline style in review: "the legend uses an inline `style` key that the CSP forbids") | TEXT+T |
| 21 | the scoring site uses a different seed (§8) | TEXT (dynamic field and board discovery) | TEXT ("nothing is hardcoded to the dev seed") |
| 22 | "You never call the site directly" (STARTER) | violated once (C22 curl), then "I'm going to stop probing the dev site with direct `curl` calls." | complied |

What the table shows:

- Every semantic trap (rows 1 to 6, 8 to 11, 16 and 17) was handled from the text by both models, first time. Haiku's design
  notes read like a paraphrase of the contract. For example, contract §3 "a run with nothing new writes nothing"
  became "I'm reasoning that writes should only happen when a state value actually differs from what's stored, since
  idle runs with nothing new are supposed to write nothing — so I'll compare-before-write for state and rely on
  FAIL_IF_EXISTS to dedupe change rows for free."
- No semantic defect was found by testing in either app. All of Haiku's failing runs were wiring: scopes, manifest
  shape, consumer shape, resolver references, the KVS page limit and the ADF request body. Each one surfaced as an
  explicit, greppable message from lint, the dev Jira or the emulator.
- Haiku's self-review introduced a bug that only the dev site caught. C25: "The comment POST wraps the ADF doc one
  level too deep: the request body must be the doc itself." C58: "The comment post fails with Jira's 400 because my
  ADF body is sent as the body directly, and Jira needs it wrapped in `body`." It then misattributed that 400 to the
  seeded refusal: "The 400 is also the dev site's denied-comment case, which the viewer `users` list names (CORE-400
  for the default viewer), so the viewer-permission path is what this run tests." That is false: the 400 came on
  CORE-448, and the cause was the body shape.
- STARTER named two dev-site fixtures that exist to be tested: the comment-refusing issue and the scripted LLM.
  Pareto used both on purpose. Haiku used neither, and the scorer could not tell the difference: both get 8/8 on
  comment flow and 23/23 on the LLM explanation.

---

## 3. (c) Budget

| | Haiku 5.5 | Pareto 26.10 |
|---|---|---|
| calls used / budget | 72 / 150 (48%) | 50 / 150 (33%) |
| tool calls | 99 (parallel batches of up to 7) | 50 |
| agent wall | 1,204.8 s (20.1 min) | 617.5 s (10.3 min) |
| prompt tokens (cached) | 6,925,493 (6,767,133) | 3,170,693 (2,988,371) |
| output tokens (reasoning) | 206,691 (150,848) | 31,085 (none reported) |
| billed, lower bound | $0.52 | $0.34 |
| `verdict.budget.stopped_by` | `model_finished` | `model_finished` |

Neither model ever saw a call counter. The turn-context only reports tokens, for example "context: 149,847 of
1,000,000 tokens used (15%)".

Haiku's words about its budget were inconsistent and anxious at low usage:

- "keeping in mind I have a budget of 150 model calls to work with efficiently" (C1)
- "Context is getting low, so I should move toward building soon." (C19, at 64k of 1M tokens)
- "Context usage is fine at 7%." (C21)
- "I'm burning through context on API exploration and only at 8% remaining" (C24, when 8% was USED)
- "Given my remaining budget, I want to quickly verify dark mode and the 380px layout…" (C67)
- "I'm tracking my remaining call budget and still need to verify the sprint modal's action flow like sorting and
  posting work correctly." (C68)

It never did that sorting/posting browser check. It stopped at 72 calls with this summary: "The Scope Ledger app is
built and wired into the workspace. `npm run lint` reports no issues, the Custom UI bundles build, and the dev-site
checks I ran passed. Some scored behaviour is unverified, listed below." The list includes "Permission filtering was
not tested with a user who cannot browse some issues, so the hidden-count path is unverified." It closes with an
admission: "I used `curl` against the dev site once early on, which STARTER.md says the app must not do."

Pareto never mentioned the budget. Its stop: "I haven't found a mismatch that warrants a handler change, so I'll stop
testing, document the handoff, and shut down the temporary servers." Final summary: "The Scope Ledger app is complete
and verified. All temporary dev servers are stopped." Its stated limitations: verification on the dev seed only, the
widget config shape as the dev host documents it, and estimation `none` treated as 0.

For the remote models only wall times exist: Sol 791.5 s, Opus 5.5 1,587.7 s, Sol Pro 1,328.3 s. Their call counts
are not on this machine. At this budget, the call limit is roughly 2x to 3x what the frontier uses.

---

## 4. (d) Residual headroom from the board

Composition (score_forge.py): `final = 0.88 × inner + 0.12 × (excellence fraction × E mean)`. Here `inner` is the
weighted mean of tiers L/K/T/R/S/B/U/V/A, and E mean is the mean of three rows: `e_reconcile_economy`,
`e_event_economy` and `e_ui_round_trips`. For the top four, inner = 1.0 and the excellence fraction = 1.0.

| model | score | rows below 1.0 (of 66) | detail (mean over 3 scoring sites) | points lost |
|---|---|---|---|---|
| GPT-6.1 Sol | 0.9769 | `e_event_economy` 0.4225 | 98 Jira calls / optimum 28 = 3.50x; 1.0 at <= 1.46x | 0.0231 |
| Opus 5.5 | 0.9767 | `e_event_economy` 0.4171 | 90 / 27 = 3.33x | 0.0233 |
| Sol Pro | 0.9664 | `e_event_economy` 0.4096; `e_reconcile_economy` 0.75 | 105 / 29 = 3.62x; backfill 18 / 15 = 1.20x | 0.0236 + 0.0100 |
| Haiku 5.5 | 0.9661 | `e_event_economy` 0.4028; `e_reconcile_economy` 0.75 | 91 / 24 = 3.79x; backfill 16 / 15 = 1.07x | 0.0239 + 0.0100 |
| Pareto 26.10 | 0.9450 | `e_event_economy` 0.3950; `r_as_app` 0; `r_completes_in_timeout` 0 | 85 / 22 = 3.86x; both R rows "vacuous — precondition unmet: a scheduled trigger made >= 1 Jira call" | 0.0242 + 0.0308 |

Reading it:

- The residual is excellence-only for the top four. Every stated behaviour scored 1.0, from duplicates, reordering,
  loss, pagination, 429, permissions, comments, LLM, realtime, CSP and dark mode to the Rovo action, skill and MCP.
  There is no stated behaviour left to gain on.
- The top-4 spread (0.0108) is the reconcile-economy rung: 14 calls (Sol, Opus) scores 1.0, while 16 or 18 calls
  scores 0.75. The composition audit already logged this as defect D3.
- Pareto is where it is because of a scorer artifact. `scheduled()` only enqueues `{ source: 'reconcile' }`, and the
  consumer (`timeoutSeconds: 900`, `concurrency: { key: 'ledger-writer', limit: 1 }`) does the work. The two R rows
  look only at invocation kind `scheduled`, so they go vacuous and score 0. By the contract's own wording ("Background
  work uses asApp()") the design is compliant, and it is the more robust one at scale (see 5.2).
- The economy optimum is reachable and craft-specific, but the contract does not state it. The golden reads once per
  delivered change: `GET /rest/api/3/issue/{id}?fields=…&expand=changelog`, recording only the delivered changelog
  entry, with no read for a duplicate (golden-forge/src/sync.js: "One read: the issue's current Sprint and estimate
  values AND its changelog… Only that entry is recorded: lost siblings are the scheduled run's to heal."). The top
  models read the issue plus `changelog/bulkfetch`, which the scoring site serves in two pages, so 3 reads per
  delivery, and they read again on duplicates. On Haiku's seed 128034, of 31 consumer invocations, 21 made 3 Jira calls,
  7 made 2, 2 made 4 and 1 made 6. Qwen3.8 Flash (36 calls, 1.29x) and DeepSeek V4.1 Flash (32 calls, 1.33x) both
  scored 1.0 on this row, so the bar is reachable, and two much weaker models reached it. The top five simply did not
  aim at it.

Saturation is a top-5 effect. Number 6 on the board scores 0.7978, so Forge 1.0 still separates the field below the
frontier. Among the top five it measures only the unstated economy plus one scorer artifact.

---

## 5. (e) Judgment: what would defeat or seriously slow these models

### 5.1 What the transcripts show about how they win

- The contract works as a test plan. Each injected fault is announced: duplicates, reordering, loss, Retry-After,
  "the harness runs the scheduled job once before it delivers any update", the LLM's three failure shapes, the
  double click, "Trigger `filter.expression` is not evaluated", the redelivery schedule. STARTER even names the
  comment-refusing issue and the scripted LLM. The models' designs are paraphrases of those sentences, and the
  paraphrase is enough: Haiku passed every row it never exercised.
- The dev tools act as a wiring linter with explicit messages. Haiku's development loop was "write it all, let the
  tools say what is wrong": six defect classes, each surfaced by a distinct string. It found the KVS limit by grepping
  the emulator source (`grep -rn "page limit must be set" lib/`). Pareto needed none of that.
- The world is small and stays put. Per the pack model in forge_oracle.py, boards, sprints (fixed `state`), issues and
  estimation fields are static, and the live stream is issue changelog entries with delivery modifiers (duplicates,
  dropped, liveUi). A scrum board without an estimation field is a pack defect, so "no value counts as 0" at the board
  level is never exercised. Consumers ran strictly one at a time: on Haiku's seed, 31 consumer invocations had a
  maximum overlap of 0, measured from their call timestamps. The longest Retry-After was 30 s ("waited 30.0 virtual s
  in-invocation"). Haiku's whole backfill took 4.5 s against a 55 s default timeout.

### 5.2 Requirement kinds that plausibly defeat or seriously slow them (ranked by my confidence)

1. **Concurrency with a throughput bar (high confidence).** Evidence: Haiku saw the race, then accepted it. In C18:
   "I'm accepting the eventual consistency tradeoff here since concurrent writes both read fresh state anyway". Its
   `queue.push` has no concurrency key, and its `sprint-issue` upsert is read, compare, write. An older read can
   overwrite a newer one when a sprint move and a re-estimate for one issue run in parallel. Pareto avoided the race
   by serializing the whole site through one consumer (`limit: 1`), which is correct but has a throughput ceiling.
   Requirement: deliver same-issue updates concurrently and out of order, and require a burst (say 2,000 updates) to
   be ledgered within N harness-minutes. Racy designs then produce wrong numbers, global serialization misses the bar,
   and only partitioned serialization or conditional writes pass. This is fair: Forge documents the queue concurrency
   option and KVS conditional writes.
2. **Scale against real platform limits (high).** Evidence: both reconcile designs rescan every issue updated since
   the earliest active sprint started, site-wide and unscoped, every hour (Haiku's JQL `updated >= "2026-11-03
   07:02"`; Pareto's `(updated >= … OR sprint in (…)) ORDER BY key`). Both do per-key KVS reads with no batch
   operations: Haiku's no-op rerun issued 458 entity gets for 95 rows, Pareto's 163 KVS calls. Haiku runs the whole reconcile
   inside the scheduled function on the 55 s default (no `timeoutSeconds`). It had designed the fix and dropped it.
   C18: "My plan: persist a `lastRunAt` watermark and query for issues updated since `lastRun - margin`…" The shipped
   code uses `Math.min(...startDates) - 2 * DAY` every run. Forge KVS allows 1,000 RPS and 4,000 10 KB read units per
   minute per installation. At 50x the scoring site (about 5,000 rows, about 150 active sprints) Haiku's hourly run
   needs at least about 23,000 KVS reads (the count grows with issues × active sprints), about six minutes of read
   quota, so it cannot finish in 55 s. Pareto's single 900 s consumer survives longer but not
   indefinitely. Requirement: a site big enough that the backfill must be chunked across invocations with a resumable
   cursor through the queue, with the hourly heal incremental.
3. **Retries and side effects under ambiguous failure (medium-high).** Evidence: both sleep through Retry-After inside
   the invocation (Haiku: "which works fine since consumer invocations can run up to 900 seconds", while never setting
   `timeoutSeconds`). Haiku parses numeric seconds only (`Number(res.headers.get('retry-after')) || 1`), so an
   HTTP-date header would wait 1 second. Comment de-duplication lives only in the UI. Pareto's comment says so: "Never
   retry a successful POST; the UI coalesces a double click before invoking this resolver." Haiku's: "rather than
   adding backend-side deduplication". The harness only injects a clean 429 on the comment POST. Requirement: a
   Retry-After longer than the remaining invocation time, which needs the `InvocationError`/`retryAfter` path that
   contract §8 mentions and nobody used. Add a comment POST that Jira commits but answers with 5xx or a timeout, a
   second tab, and two viewers acting on the same change. Each needs server-side idempotency.
4. **A world that changes mid-run (medium-high).** Evidence: both rely on the harness guarantee in row 7.
   - Haiku caches the Sprint field and each board's estimation field forever (`if (key in cfg.boardEst) return;`), so
     an admin switching a board's estimation field is never noticed.
   - Pareto's event path only knows the sprints in its hourly `ledger-config` snapshot, so changes to a sprint started
     since the last run are not recorded live.
   - Pareto's consumer throws on a 404, so a deleted issue is redelivered for 24 hours.
   - Neither re-derives rows when a sprint's start date is edited, and both store issue keys, which go stale after a
     move.

   Requirement: during the live phase, start and close sprints, edit a start date, switch a board's estimation field,
   delete and move issues, install with events flowing before the first scheduled run, and require correct live
   numbers plus convergence by the next run. Fair if each behaviour is real, documented Jira behaviour.
5. **Live migration of installed data (medium for "seriously slows", high for "a kind they have not been tested
   on").** Neither app versions its storage. There is no schema-version key and the entity shapes are fixed, so both
   treat KVS as a fresh store. Requirement: v1 is installed with a populated ledger, and v2 must change the row shape
   (for example, per-change estimate snapshots) while events keep flowing, with zero rows lost or double-counted
   through the cutover. This needs dual-read, backfill and cutover reasoning that no Forge 1.0 instruction prompts.
6. **State invariants, not the fault list (medium-high, and it multiplies items 1 to 4).** Today's contract enumerates
   exactly what the harness will do. If Forge 2.0 states only the invariant ("the ledger is exactly right under
   Forge's documented delivery semantics and Jira's documented behaviour") and ships those docs offline, the model
   must know or derive the failure modes. Fairness rule: every injected behaviour must be documented in the offline
   reference. Otherwise it becomes an unstated rule (see 5.3).
7. **A dev site that does not announce every error (medium; separates Haiku-style from Pareto-style).** All six of
   Haiku's defects were caught only because the dev tools printed them. If some failure classes behaved like
   production instead, for example a scope gap only on a code path the dev data never reaches, or wrong numbers
   rather than an error, "write it, let the tools tell me" ships defects and first-time-right does not. I cannot tell
   which style Sol and Opus use.

### 5.3 Kinds of difficulty that would not challenge them

- Pure volume (more modules, surfaces, files). Pareto wrote 11 source files in 11 calls, lint-clean the first time.
  Haiku emitted 5 backend files in one response. Both used under half the budget.
- API and manifest trivia. It was resolved from the offline schema, OpenAPI and typings with 0 failing lint runs
  (Pareto) to 3 (Haiku, repaired in 5 calls).
- Precise numeric, ordering and formatting rules. All rows scored 1.0 (`u_widget_numbers` 4/4, `u_ledger_table` 3/3,
  tie-breaks, rounding).
- More precisely specified UI/LLM behaviour. Haiku passed 23/23 LLM checks having run only the clean answer.
- Permission changes mid-run. Both compute visibility per request as the viewer (Haiku `visibleKeys` as asUser,
  Pareto `search(…, true)`), with no cache to go stale.
- Different seeds or renamed fields. Both discover the Sprint and estimation fields at runtime.
- A tighter call budget or wall clock on its own. Pareto finished in 50 calls and 10 minutes, Sol in 13 minutes. A
  limit would mostly punish exploration style. Haiku found the KVS-limit bug, which broke every read path (widget,
  modal, Rovo), at call 50. Had the session been cut there, the code as it stood would have shown no ledger data,
  which caps the score at 0.699. That is a speed test, not a depth test.
- Unstated grader preferences. `e_event_economy` is the live proof: all five sit between 0.395 and 0.4225 because none
  of them guessed an unstated optimum. Meanwhile Qwen3.8 Flash (0.7961 overall) and DeepSeek V4.1 Flash (0.3354
  overall) scored 1.0 on that row. A hidden preference rewards incidental design, not capability, and it disappears once
  the target is written down. If economy matters, state it ("at most one Jira read per delivered relevant change; none
  for a duplicate").
- A second product. Mostly volume, I judge: both solved permissions with one transferable pattern (search as the
  viewer), and both read unfamiliar APIs off the offline OpenAPI in a handful of calls. Low confidence that it
  separates unless the two products' data interact, and then it becomes items 1 or 4 above.

### 5.4 Scorer lessons to carry into Forge 2.0

- Grade the behaviour, not the invocation kind. Pareto lost 0.031 for the platform-idiomatic design (scheduled
  trigger, then queue, then a 900 s consumer) that would survive scale better than the designs that scored 1.0.
- `r_completes_in_timeout` is trivially satisfied at this scale (4.5 s against 55 s). It only means something on a
  site big enough to threaten the timeout.
- Make reconcile economy continuous (D3). As it stands, one call is the whole gap between the top two and #3/#4.
- Seeded dev fixtures (the refusal issue, the scripted LLM) are good, but in 1.0 they reward only the contract text.
  In 2.0 each fixture should guard a behaviour that the text alone does not determine.

## 6. Confidence

- High: (a) to (d). They come straight from sessions.db, telemetry, verdict.json, forge-observations.json, the final
  source and the board rows. The per-activity bucketing is my classification of each call. The bucket edges are
  judgement, but the call lists are in `frontier-work/*-calls.txt`.
- Medium: (e) as applied to Sol, Opus 5.5 and Sol Pro. Their trees are not here, so whether they share Haiku's
  unguarded concurrency, in-function reconcile or UI-only comment de-duplication is unverified. They could already use
  per-issue queue keys or server-side idempotency. The claims about the harness world hold for every entrant as far
  as the shared harness goes: static config comes from the pack model in forge_oracle.py, the 30 s maximum
  Retry-After appears in all five board rows, and sequential delivery was measured on Haiku's seed.
- Not done: no scorer, browser or build was run (CPU rule), so the scale and concurrency failure predictions are
  reasoned from code and measured call counts, not reproduced.
