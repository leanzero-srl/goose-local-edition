# Forge 2.0 golden (P10, 2026-10-09): what the v2 golden decides where SPEC §1-§2 leaves room

Built from `forge2/SPEC.md` §1-§2 and the verified research only (P6's contract text was not read). Each line is
an interpretation the contract (P6), the oracle (P8) and the probe (P9) must match, or the golden must change.

- **R7 field API: `POST /rest/api/3/app/field/value`**, not the `PUT` SPEC §1/§2.1 name. The shipped OpenAPI
  (`forge2/kit/openapi/jira.json`) has the bulk update as POST (`MultipleCustomFieldValuesUpdateDetails`:
  `{updates:[{customField, issueIds:[int], value}]}`); PUT exists only as `/app/field/{fieldIdOrKey}/value`. The
  golden sends `?generateChangelog=false&generateAppEvents=false`, ≤ 200 issue updates per request, empty = `null`.
- **R7 field discovery:** `GET /rest/api/3/field`, the field whose `schema.custom` ends `/scope-status` (the extension
  ARI `…/static/scope-status`) or whose `key` ends `__scope-status`. None found = logged, statuses not written.
- **R7 values:** `committed` / `added +<points>` (the points of the change that last added it: the current value of
  the field its board used at that change, contract §1/§15) for the active sprint the issue is in; `removed` when it left an active sprint after its start and is in none now; empty
  otherwise (incl. after its sprint closed). Deleted issues are not written.
- **R4 deleted issue:** rows get `deleted: true`; the membership keeps its last state with `deleted: true`, so the
  issue counts as not-in-now: still in `committed` if it was in at start, and in `removed`. Its rows are hidden from
  every person (no one can browse a deleted issue) and counted in hidden-count.
- **R4 closed sprint:** no row with a change time after `completeDate`; the event path reads each sprint and board
  fresh (one agile GET each per invocation), so closes and estimation-field switches apply at once.
- **R4 estimation-field switch** (settled by contract §1, 2026-10-10): rows keep the estimate they were written
  with; the totals use the board's current field; a change's points (the modal's points column, the Rovo action,
  `added +<points>`) are the current value of the field its row records (`estimateField`, the field the board used
  at the change). Members carry every estimation field's current value (`estimates`, JSON) for that. A closed
  sprint's members stop updating, so its numbers and points stay as at the close (§12).
- **R1:** the v1 key (`<changeId>:<sprintId>`) is the v2 key, so copies are exactly-once by construction. Migrated
  rows: estimate = the issue's current value of the sprint's board field (v1 stored none), `deleted` if Jira no
  longer has the issue. Progress text: `Migrated <n> of <total> v1 rows`, `… — complete`, before the first step
  `Migration has not started yet`. Starts on `avi:forge:upgraded:app` and on every scheduled run.
- **R2 dose:** each background invocation records its spend under KVS `dose:<hour>:<invocation>` and sums the
  hour's keys; it stops (consumer: `InvocationError`; scheduled: a delayed queue continuation) before a request
  that would pass `floor(2400 × share / 100)`. A quota 429 writes `dose-paused-until`. Needs `Date.now()` virtual.
- **R6:** static web trigger, outputs `accepted` 202 / `duplicate` 200 / `unauthorized` 401 / `invalid` 400 (a
  signed but malformed body). The handler returns `{outputKey}` AND the dynamic shape (`statusCode`, `body`,
  `headers`) so either host reads it. "Deployed to <env>" = `deployedEnvs` on every ledger row of the issue, shown in
  the sprint ledger's extra column `th[data-col="deployed"]`. A valid event before the migration completes is
  queued (still 202) and applied by the consumer once it has.
- **R8:** an LLM 429 (the SDK exposes no Retry-After) backs off 20 s, 40 s, … server-side; the page gets an error
  with `retryAfter`, never an auto-retry. Cache key = the exact prompt input (viewer-visible data only), 10 minutes.
- **R9:** `index.js` (bridge + plain DOM, ~97 KB) + `index.css` paint the first data after one invoke; React
  `app.js` is requested on the next frame and starts from `window.__scopeBoot` (no second invoke).
- **R5 labels:** each control is `<Label labelFor=id>` + the control with that `id`; buttons by their text; the
  migration status is a read-only Textfield labelled `Migration` plus the same text as a Text node.

# Contract gaps found while building the golden (WP3, 2026-10-02)

The golden was built from `forge/public/{spec-build-forge.md, FORGE-CONTRACT.md, STARTER.md}`, the installed
`@forge/*` packages and developer.atlassian.com only. Every place where the public text did not decide a
behaviour is listed here: the exact sentence, what the golden does, and what the orchestrator should settle.
Ranked by how likely the gap makes a correct entrant lose points.

## 0. Why WP2 saw zero ledger rows (2026-10-02, reproduced on forge-dev, dev seed 74949e931b820fc4)

Three defects, two on the site/kit side and one in the golden; all three would hit an entrant too.

- **SITE: changelog `created` is an epoch-millisecond NUMBER** (`"created":1791271684753` in
  `POST /rest/api/3/changelog/bulkfetch`). The Jira OpenAPI types `Changelog.created` as
  `string, format: date-time`, and real Jira returns ISO-8601 strings. `Date.parse(number)` is NaN, so an app
  written to the OpenAPI drops every change. This was the zero-rows cause. FIX IN WP1's site (serve ISO
  strings); a correct entrant must not need to guess the encoding. The golden now parses both encodings and
  logs an unreadable `created` loudly instead of skipping it silently (`instantOf` in src/sync.js).
- **GOLDEN (fixed): KVS `integer` attributes are 32-bit** (developer.atlassian.com, "Defining Custom
  Entities": integers from -2,147,483,648 to 2,147,483,647). The golden stored the epoch-ms change time in an
  `integer` range attribute; the emulator refused it ("Attribute 'at' must be of type integer"). `at` is now
  `float` (epoch ms are exact up to 2^53). The contract asks for "change time (range)" without a type, which
  is fine — but the emulator's message names the type, not the range, which misleads (suggest echoing the
  range).
- **SITE: `POST /rest/api/3/issue/{id}/changelog/list` was not modelled** (501 EMULATOR_NOT_MODELLED → 2,190
  redeliveries). The golden no longer depends on it: the event path reads the issue's sprint history through
  `changelog/bulkfetch` and keeps the entry the event names.

Also found on forge-dev:

- **PLATFORM TRAP (real Atlassian wrapper, kit `wrapper.js`)**: for a function with `timeoutSeconds > 55`
  the wrapper validates a returned retry request with `Buffer.byteLength(JSON.stringify(retryData))`, which
  THROWS when `retryData` is absent, turning `new InvocationError({retryAfter, retryReason})` into a function
  error (redelivered on the 1-minute schedule instead of after Retry-After). The golden now always passes
  `retryData`. This is real platform behaviour, so it stays a trap — but nothing public mentions it; decide
  whether the contract should.
- **KIT: concurrent `forge-dev serve` processes in one workspace corrupt `.forge-dev/state.json`**
  (`SyntaxError: Unexpected end of JSON input` in `loadState`, from the periodic `saveState`; the served page
  then fails with ERR_INCOMPLETE_CHUNKED_ENCODING). STARTER tells entrants to start serve in the background,
  so two at once is a natural move. Fix: atomic write (temp file + rename) or one state file per serve.

Verified on forge-dev after the fixes: backfill 52 rows (all `reconcile`) and 87 memberships; 60 site updates
delivered → +25 rows `event`, one 429 on the consumer answered with a retry request and redelivered after
exactly its Retry-After (30 s); heal +4 rows `reconcile`; rerun 0 KVS writes, 0 queue pushes; Rovo action
for four users: same team totals (62 / 18.5 / 15 / 29.8), hidden counts 1/0/1/0; widget (dark), edit and
sprint action render with the bridge host; a double click on post-summary = one invoke, one flag.

## A. Gaps that can cost a correct entrant points

1. **Which JQL and endpoints the site serves.** STARTER: "It behaves like Jira Cloud for the calls this app
   needs". The golden needs, and therefore the site must model:
   `POST /rest/api/3/search/jql` with `sprint in (11, 12) OR updated >= "2026-09-20"` (OR, `sprint in (ids)`,
   `updated >=` a date), `POST /rest/api/3/changelog/bulkfetch` with `fieldIds` (backfill AND event path),
   `POST /rest/api/3/issue/bulkfetch` as the user (Jira leaves out issues the user cannot browse),
   `GET /rest/api/3/issue/{id}?fields=…`, `GET /rest/api/3/field` (the Sprint field found by
   `schema.custom = com.pyxis.greenhopper.jira:gh-sprint`), `GET /rest/agile/1.0/board?type=scrum`,
   `/board/{id}/configuration` (`estimation.field.fieldId`), `/board/{id}/sprint?state=active`,
   `/sprint/{id}`, `POST /rest/api/3/issue/{id}/comment`. Finding issues that "have since left every
   sprint" (§3) needs a JQL restriction other than the Sprint field (Jira has no `sprint WAS`); `updated >=`
   is the real-Jira way. If the site's JQL subset lacks `updated` or `OR`, the golden's backfill fails.
   Settle: state in STARTER that the site supports the JQL operators Jira Cloud supports, or name the subset.

2. **onProductSave's argument.** §4: "The dashboard's own Save stores what your `onProductSave` handler
   returns (`null` stores nothing)". The contract does not say what the handler RECEIVES (the last
   `updateConfig` value? the stored config?). The golden returns `{...argument, boardId: <its own selection>}`
   and `null` when nothing is selected, so it is right under either reading. An entrant that returns its
   argument unchanged is right only if the harness passes the last `updateConfig` value. Settle: one sentence.

3. **A Retry-After longer than a resolver can live.** §5: "On `429`, retry after `Retry-After`: each click
   (or double click) ends with exactly one comment and one success flag." Resolvers time out at 25 s
   (§8 "platform's timeouts"). The golden sleeps in the resolver up to 5 s and hands a longer wait back to the
   page, which waits and calls again. Settle: name the longest Retry-After the harness sends on this path.

4. **Waiting inside a scheduled run.** §3: "wait at least that long before the next attempt" and §8 "Waits
   are on the harness's virtual clock" (said of redelivery). The golden's scheduled run sleeps in real time
   (setTimeout) for a 429 on search. If the harness measures the gap on its virtual clock, a real sleep shows
   as 0. Settle: say how a wait inside one invocation is measured.

5. **"Updates that touch neither do no Jira or queue work."** §3. The trigger needs the estimation field ids
   to recognise an estimate change; the golden reads them from its own KVS config (one KVS read, no Jira, no
   queue). Before the first scheduled run there is no config: the golden then recognises the Sprint field by
   its changelog `field` name `Sprint` and drops estimate-only updates (the first backfill reads current
   estimates anyway). Settle: say KVS reads are allowed there; say whether events arrive before the first
   scheduled run.

6. **Which path "recorded it first".** §3: "Each row records `source`: `event` or `reconcile`, the path that
   recorded it first." When the consumer handles an issue, it could also record that issue's OTHER changes
   whose events were lost. The golden records only the changelog entry the event names; lost siblings are
   left to the scheduled run (`reconcile`). Settle: "an event records only its own changelog entry".

7. **Estimate of an issue across boards.** §1: "An issue's estimate is the value of its board's estimation
   field". An issue can sit on several boards; a sprint can show on several boards. The golden uses the
   estimation field of the sprint's `originBoardId` for that sprint's numbers, on every widget that shows it.
   Settle: "the estimation field of the board the sprint belongs to (originBoardId)".

8. **Sorting edge cases.** §5: "Clicking `th[data-col="at"]` toggles descending/ascending". From the points
   sort, the golden's click on `at` goes to `at` ascending. Descending is the exact reverse of the default
   order (equal times by changelog id DESCENDING). "changelog id ascending" is compared numerically. Settle all
   three in one sentence.

9. **"Nothing clips".** §4: "nothing clips or scrolls horizontally from 380 to 1180 px". Read strictly this
   forbids ellipsis truncation of long names; the golden wraps instead of truncating. Settle: allow or forbid
   ellipsis explicitly.

10. **Contrast of disabled controls.** §7: "contrast at least 4.5:1 in light and in dark". Disabled controls
    are exempt in WCAG, but the sentence has no exemption; the golden therefore never renders a disabled
    control (post-summary with no selection shows an info flag instead). Settle: exempt or not.

11. **Scopes "your calls need" for Jira Software.** §2. The OpenAPI lists TWO granular scopes jointly for
    `GET /rest/agile/1.0/board` (`read:board-scope:jira-software` + `read:project:jira`) and for
    `/board/{id}/configuration` (`read:board-scope.admin:jira-software` + `read:project:jira`). The golden
    declares both. Also MEASURED: client-side lint does not flag missing scopes when the path goes through a
    helper (removing `write:jira-work` and `read:sprint:jira-software` from the golden: "No issues found"), so
    lint cannot guide the entrant here. Settle: confirm the needed-set is derived from the OpenAPI security
    entries of the calls made.

## B. Gaps where the golden's choice is unlikely to be contested

12. Sprint action on a closed sprint: §1 defines numbers "for each active sprint". The golden shows whatever
    the ledger holds (rows recorded while it was active).
13. `not-started` and "nothing else" (§5): the golden renders no close button there (the host's own modal
    close remains). Settle if `close` is expected on that screen.
14. Rovo action for a future sprint (§6 names only unknown/missing ids): the golden returns the object with
    zeros and `creepPercent: null`. A non-numeric `sprintId` returns `{error}` without a Jira call.
15. Router (§5 "through the Forge router"): the golden uses `router.open('/browse/KEY')`, bridge op
    `navigate` with `type: 'new-tab'`; `router.navigate` would send `type: 'same-tab'`. The harness should
    accept both.
16. post-summary with no row selected: unspecified; the golden shows an info flag and posts nothing.
17. "a run with nothing new writes nothing" (§3): the golden makes zero KVS writes and zero queue pushes, and
    rewrites its config only when sprint metadata actually changed (a renamed sprint is "new").
18. `selfGenerated` issue updates: unspecified; the golden writes no issue fields, so it processes them like
    any other update.
19. Equal sprint `startDate` (§4 "ordered by startDate"): ties by sprint id.
20. A change at exactly `startDate` (§1 "created after startDate"): excluded (strictly after).
21. STARTER: `npm run lint` comes from the workspace `package.json`; the golden's script is a placeholder
    (`node $FORGE_KIT/bin/lint.cjs`) until WP1's starter fixes the real one. Align it with `forge/starter`.

## C. Corrections to DESIGN §13.5 (measured while authoring the mutants)

22. `m_skill_name`: the table says `k_rovo_skill`, ≤ 0.799. MEASURED with the client-side lint
    (`forge/spike/harness/lint-offline.cjs`): ERROR "Skill sprint-scope-analyst frontmatter field 'name' must
    match the parent directory name", so the lint band applies: `l_deployable` + `k_rovo_skill`, ≤ 0.499.
    The expectation file says so.
23. `m_storage_api`, `m_old_search`, `m_abs_assets`, `m_gadget`: the defect stops a whole phase (no ledger, no
    backfill, no widget). The expectations list the rows §13.5 names; every downstream row that reads the
    broken phase must be attributed by `ROOT_BLOCKS`, which WP3 has not seen. `m_string_comment` and
    `m_asapp_ui` additionally declare `b_comment_exactly_once` and `b_hidden_count` / `a_action_permissions`,
    which the same defect breaks directly (no comment at all; a hidden count of 0).
24. `m_double_post` only bites if the post button stays under the pointer: a label that changes width while
    posting ("Posting…") moved the button so the second click of a double click landed on the container
    (measured on WP3's bed). The golden keeps the label fixed; the in-flight guard is what makes it pass.

## D. Forge LLM, Realtime and rovo:mcp (contract 2006de559, verified on forge-dev kit-db300f0d5da47478)

25. **Realtime tokens from async functions: the public docs disagree.** "Authorizing Realtime channels" says
    async functions (queue consumers, triggers) must sign a token with the same claims as the subscriber and
    publish with it; the Realtime events API page says "import the `signRealtimeToken` function into your
    resolver" and calls `publishGlobal`'s token optional. Per the coordinator's correction the golden signs
    nothing: consumer and scheduled run `publishGlobal({sprintIds})` with no token; the widget
    `subscribeGlobal`s with no token. Settle in the contract: "async publishers use `publishGlobal` without a
    token" (or the opposite), since an entrant following the authorizing page writes the other design.
26. **Sampling parameters.** MEASURED on forge-dev: `chat` with `temperature` answers 400 "claude-sonnet-5 does
    not support the temperature and top_p sampling parameters". The @forge/llm README's own example sends
    `temperature` and `top_p`, so an entrant copying it fails every explain. If this mirrors the real
    platform it is a fair trap; if it is the emulator's choice, the contract should say so.
27. **`tool_calls[].function.arguments` type.** @forge/llm types it as `object`; the golden treats anything else
    (including a JSON string) as malformed. Settle whether a string-encoded object counts as malformed.
28. **Which active model.** The contract says "a model that list() reports active"; with several active the
    golden prefers a Sonnet, else the first active one.
29. **Explanation after a failed attempt.** The contract says a refusal/malformed/error shows an error flag
    and leaves the modal working; the golden also clears a previous explanation so nothing on screen claims
    to answer the failed request.
30. **Global channel exposure.** A token-less global channel can be subscribed by any user of the app
    installation who knows its name (authorizing page). The payload is sprint ids only, which is why the
    golden accepts it; say in §4 that this is the intended trade-off.
31. **Points sort** is no longer required; the golden keeps clicking `points` (descending, ties by `at`) and the
    `at` toggle starts ascending from it, as §5 now states.

Verified on forge-dev (dev seed c420dda4e034a3db): backfill 65 rows; events publish `{sprintIds}` with
publishGlobal from the consumer; an open widget (board 151) moved sprint 689 from 43/97.7% to 56/127.3%
added/creep with 0 reloads after one realtime delivery, subscribed once, clean console; explain through the
scripted sequence: clean -> summary + 3 visible ids; digits -> the ledger's own sentence, unknown/hidden ids
dropped (1 kept); refusal, malformed, error -> 3 error flags, no explanation; clean again -> summary.
`npm run lint`: No issues found.

## E. Freeze-gate round (WP2 fe6e67f84 / 23012c899)

32. **Resolvers that throw.** FORGE-CONTRACT.md does not say a resolver must not throw. The only sentences are
    §5 "A failure shows an error flag and leaves the modal working" (the golden's page always did) and §6's
    "does not throw" for the Rovo action. WP2 still scores a thrown resolver as a failure. The golden now
    answers every failure as `{ok:false, error}` (Jira 4xx/5xx, KVS, anything else). Settle in §5: "resolvers
    answer errors; they never throw".
33. The dev site has no user who is forbidden to comment (all six dev users got 201 on forge-dev), so an
    entrant cannot meet the scoring site's 403 before it is graded. Either seed one on the dev site or name the
    case in §5.

DESIGN §13.5 rows that must change (verified against each mutant and the bed or forge-dev):
`m_config_in_kvs` drop `k_widget_edit_bridge` (the edit API stays in use); `m_ids_only` = `u_widget_numbers`,
`a_action_result`, not critical, lands 0.799 (rows complete); `m_one_estimate_field` add `t_trigger_handoff`;
`m_retry_now` add `r_backfill_complete` (crit) + `r_removals_found`; `m_runtime18` add `k_current_apis` and mark
`l_deployable` crit; `m_storage_api` drop `r_pagination` (its scheduled run makes zero Jira reads on forge-dev);
`m_llm_deprecated_model` -> `m_llm_unknown_model` (`u_llm_explain`, `k_llm_model_current`).
- `m_llm_no_refusal_path` re-authored after the never-throw wrapper: a refusal renders the raw model text as the explanation with no error flag; expected loss `u_llm_explain` only (drop `b_invoke_contract` from the §13.5 row).
