# Contract gaps found while building the golden (WP3, 2026-10-02)

The golden was built from `forge/public/{spec-build-forge.md, FORGE-CONTRACT.md, STARTER.md}`, the installed
`@forge/*` packages and developer.atlassian.com only. Every place where the public text did not decide a
behaviour is listed here: the exact sentence, what the golden does, and what the orchestrator should settle.
Ranked by how likely the gap makes a correct entrant lose points.

## A. Gaps that can cost a correct entrant points

1. **Which JQL and endpoints the site serves.** STARTER: "It behaves like Jira Cloud for the calls this app
   needs". The golden needs, and therefore the site must model:
   `POST /rest/api/3/search/jql` with `sprint in (11, 12) OR updated >= "2026-09-20"` (OR, `sprint in (ids)`,
   `updated >=` a date), `POST /rest/api/3/changelog/bulkfetch` with `fieldIds`, `POST /rest/api/3/issue/{id}/changelog/list`,
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
