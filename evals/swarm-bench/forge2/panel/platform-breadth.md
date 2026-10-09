# Forge 2.0 panel design: PLATFORM BREADTH ("Launch Control")

Designer angle: the widest coherent combination of the latest Forge modules and APIs, where every module does graded
work and none is decorative. Written 2026-10-09 by one of six independent designers. I did not read the other panel
files.

Inputs read: `forge2/NOW.md`; `research/BRIEF.md` (whole); `research/{tiers,uikit,bootspeed,security,llm,robustness,
platform-2026-10-09}.md` (the design-relevant sections); `research/understand/{frontier-behaviour,real-forge-fidelity,
machinery,integration}.md`; `forge/DESIGN.md` (§0-§17.8); `research/forge1-review/*.md`; and the 1.0 public files.

Two primary sources were checked this session, both read-only:
- The pinned manifest schema `@forge/manifest` 13.6.0, in the kit cache `825be630e817fabf/schema/manifest-schema.json`.
- The shipped Jira OpenAPI, `forge/kit/openapi/jira.json`.
Facts taken from them are marked **[S]** (schema) and **[O]** (OpenAPI). Every other fact cites the brief's tag
(`tiers#N`, `rob#N`, `sec#N`, `llm#N`, `uikit#N`, `boot#N`, `[B]`) or a changelog key.

Nothing was run except small Python reads of those two JSON files.

---------------------------------------------------------------------------------------------------------------------

## 0. The design in fifteen lines

1. **Product.** *Launch Control* is release-risk control for Jira Cloud. For every issue in an active release (a Jira
   version) it computes a risk level from three inputs: cross-project blockers, CI results and scope added after code
   freeze.
2. **Where the level appears.** In seven places, each with a different platform contract:
   - a stored, searchable custom field;
   - a `releaseRisk()` JQL function;
   - a workflow validator that gates the move to Done;
   - a live Release Radar (`jira:globalPage`, Custom UI);
   - an issue panel with an AI explanation (Forge LLM, async, streamed back to the requester through Realtime);
   - a dashboard widget;
   - a Confluence static macro.

   Rovo actions, a skill, an agent and an MCP server answer the same questions. A UI Kit 2 admin page and a UI Kit 2
   project-settings page govern the app. CI/CD reports build results through an HMAC-signed static web trigger.
3. **Breadth.** 19 manifest module types, against 1.0's 10, plus Forge SQL, the Custom Entity Store, Realtime,
   app-managed permissions (Preview), the personal-data API and lifecycle events. Four modules are Preview:
   `rovo:skill`, `rovo:mcp`, the workflow validator and static macros.
4. **Why breadth is hard here, not just long.** The same fact, "issue X is HIGH risk", must hold on surfaces whose
   platform contracts conflict:
   - Jira caches JQL-function answers for all users.
   - Confluence caches static-macro output for all viewers, with no user in the cache key.
   - The custom field is shown to everyone who can see the issue.
   - The gate decides synchronously, on Jira's state at the moment of the transition.
   - Person-facing surfaces must filter by the viewer's permissions.
   - The macro runs in the app's Confluence installation, whose storage is separate from the Jira installation's.

   One data model and one freshness strategy must satisfy all of these at once. That is engineering judgment, and no
   checklist line can be transcribed to pass it.
5. **Where the 1.0 frontier had no answer.** The world is built on the difficulty vectors the frontier study ranked
   highest:
   - consumers run concurrently, and a throughput bar must be met;
   - backfill must stay within a shared Tier 1 budget, which forces work across invocations and hours;
   - CI bursts exceed the async push limit;
   - writes fail ambiguously (Jira applies them but answers 5xx);
   - the world changes mid-run (moves, deletes, links, releases, permissions and policy);
   - a live migration of v1 data.

   The contract states guarantees and platform facts. It never lists when or how the grader injects faults.
6. **Grading.** Offline and deterministic, on virtual time with counts only. One large world has four scored
   installations, plus simulated co-tenants that spend 20,000 points of the Tier 1 pool every hour. Two smaller worlds come from derived seeds and take the per-seed worst for
   correctness rows. The registry has 110 weighted rows, 9 criticals, 4 bands and a continuous excellence slice.
   1.0's §17.8 lessons are built in as rules.
7. **Budgets and cost.**
   - Predicted GPT-6.1 Sol score: **0.40–0.70**, point estimate about 0.55.
   - Recommended call budget: **250**. Sol needs an estimated 150–220 calls, and the shared 150 would turn the score
     into a speed test.
   - Per-run cost: Sol about $4–9; a cheap model (Luna-class) about $0.3–0.9.
   - Scoring time: about 20–30 minutes per tree.
   - Packaging added: about 150 MB for the UI Kit closure, plus 100–250 MB if the Forge SQL engine spike succeeds.
   - Reference app: about 10,500 LOC.
8. **Probe gates.** Four modules or features only enter the frozen benchmark after their live conformance probe on
   wolfaenpak passes:
   - the workflow validator;
   - the Confluence static macro;
   - Forge SQL (an engine spike);
   - app-managed permissions.

   The cut order is decided now (§B.4). The design is complete with or without each of them.

---------------------------------------------------------------------------------------------------------------------

## A. Product pitch

**Launch Control: release-risk control for Jira Cloud.**

**Who pays and why.** Organizations that ship one release from many Jira projects keep failing in the same three ways:
- An issue is closed and shipped while another team's blocker is still open.
- A late change slips in after code freeze.
- CI is red on a ticket nobody is watching.

Release managers today build saved filters, spreadsheets and Slack threads to catch these. Launch Control replaces that
with one computed, auditable risk model, visible where people already work:

| who | where | what they get |
|---|---|---|
| engineers | issue view | a **Release risk** panel: level, the blockers they can see, CI history, late-scope date, an AI explanation, and a "request exception" button |
| everyone | issue field and JQL | a read-only **Release risk** field (`LOW`/`MEDIUM`/`HIGH`/`CRITICAL`), searchable, and `issue in releaseRisk("HIGH")` for boards and filters |
| release managers | Release Radar (global page) | every active release, live readiness, at-risk counts, blockers across teams, and the exception queue |
| team leads | dashboard | a readiness widget per release |
| stakeholders | Confluence release pages | a release-status macro, safe to show to anyone who can read the page |
| approvers | issue panel, Radar | approve or reject exceptions; the decision is recorded and commented on the issue as the approver |
| workflow | transition to Done | the **Release gate** refuses HIGH or CRITICAL issues that have no approved exception |
| CI/CD | signed web trigger | build and test results, per issue |
| Rovo users | chat, Studio agents | "is release 4.2 ready?", "why is PAY-12 risky?", "request an exception for PAY-12" |
| Jira admins | admin page (UI Kit) | policy, roles, AI budget, Tier 1 consumption, CI secret rotation, failed jobs, migration status |
| project admins | project settings (UI Kit) | per-project freeze window, thresholds and approvers |

**Not a tutorial, not benchmaxxed.**
- Release gating across teams is a real Marketplace category.
- The hard parts are what a vendor's on-call engineer would actually face: the shared rate-limit pool, CI bursts,
  ambiguous write failures, moves and deletes, a v1→v2 upgrade with customer data, and permission boundaries across
  products.
- Nothing in it is a Forge sample.

---------------------------------------------------------------------------------------------------------------------

## B. Modules

### B.1 The modules, their status, why each is needed and what makes it hard

Statuses are as the verified brief §1.1 gives them, or as cited. "Rows" names the check-registry rows (§F) that grade
the module's work. None of these modules is graded only for being present.

| # | module / API | status (source) | role in Launch Control | what makes it hard (the judgment it forces) | rows |
|---|---|---|---|---|---|
| 1 | `jira:globalPage` (Custom UI) | GA; exactly one per app [B] | **Release Radar**, the front-facing app: release list, release detail, exception queue; in-app routing; live | Boot budgets counted in waves, ops and bytes (§C.6). Visibility filtering per viewer. Staying useful during a quota wall: Radar may use only stored data plus calls that cost no points (§C.4). Coalescing live refetches under the 500-invokes-per-25-s bridge limiter (boot#17) | `f_radar_*`, `b_*`, `s_permission_leak` |
| 2 | `jira:issuePanel` (Custom UI) | GA [B] | **Release risk** panel: level, factors, the viewer's visible blockers plus a hidden count, CI history, exception request and approval, "Explain risk" (AI) | Exactly-once approvals under ambiguous 5xx, double clicks and a second tab. The resolver must re-check BROWSE because `extension` is not validated (§C.3) | `f_panel`, `f_approval_flow`, `p_exactly_once`, `f_ai_flow` |
| 3 | `dashboards:widget` with `edit` | GA, CHANGE-3453; `edit` required [B] | Readiness widget per release; the choice of release goes through the dashboards edit API | Config per instance comes from `extension.config` (1.0 trap). Live updates. Store-first reads, with no Jira cost per render | `f_widget`, `k_widget_edit_bridge` |
| 4 | `jira:adminPage`, `render: native` (UI Kit 2) | GA (uikit#18, #20) | **Admin**: Policies, Roles, AI, Consumption, Integration, Jobs | Subpages are Custom UI-only, so navigation must be Tabs or the Router (Preview). Host semantics: `Form.onSubmit` has no data, UserPicker stores an object, DynamicTable sorts by cell key (uikit#11, #28, #29). Admin is enforced in resolvers, never by display conditions | `a_*`, `s_privilege_escalation` |
| 5 | `jira:projectSettingsPage`, `render: native` (UI Kit 2) | GA (platform §2.9; accepts `render: native`, uikit#18) | Project admins set freeze days, thresholds and approvers for **their** project | Authorization is bound to the same resource: ADMINISTER_PROJECTS on the project being written, checked live. `extension.project` can be forged | `a_project_settings`, `s_same_resource` |
| 6 | `jira:customField` (stored, read-only string) | GA [B]; types and `readOnly` [S] | The **Release risk** field, written by the app through `POST /rest/api/3/app/field/value` [O]; searchable | The value is shown to anyone who can view the issue, so it must name nothing hidden. Writes are dosed: one bulk update per level group costs 1 point. The app's own writes produce `updated` events, so it needs `ignoreSelf` or `selfGenerated` handling [O: `generateAppEvents`; rob#35] | `d_risk_values`, `p_self_events`, `k_field_contract` |
| 7 | `jira:jqlFunction` `releaseRisk(level)` | GA, contract captured [B] | `issue in releaseRisk("HIGH")` means level ≥ HIGH, for boards, filters and Rovo | "Precomputations are not scoped to users", so the answer must not depend on the caller. Precomputations persist until updated through `/rest/api/3/jql/function/computation` [O], or for 7 days. There is a "1,000 right-hand side values" cap, and installation L has more than 1,000 HIGH+ issues. 25 s limit [B]. An id list fails at scale. A fragment backed by the stored field (`cf[...] in (...)`) is always fresh and small. The model must find that trade-off | `d_jql_results`, `k_jql_contract` |
| 8 | `jira:workflowValidator` (function form) | **Preview** [B]; `function` in schema [S] | **Release gate** on transitions into a Done-category status | Synchronous, on the user's transition. It must decide on Jira's state now, while product events lag up to 3 min (rob#33) and the app's store may be stale. Its message is seen by the transitioning user, so it names no issues. Fails closed with a stated message | `d_gate_decisions`, `k_validator_contract` |
| 9 | `macro` with `static` (Confluence) | **Preview**, CHANGE-3505; `static.{function, concurrency, cacheConfiguration.keyComposition}` in schema 13.6.0 [S] | **Release status** macro on Confluence pages | Output is cached by `keyComposition`, which has no user dimension [S], so it is shared by every viewer and must be aggregate-only. Two macros for two releases on one page need `macro.params` in the key. The app's Confluence installation cannot read its Jira installation's storage (sec §8: "Data stored by your Forge app for one Atlassian app is not accessible from other Atlassian apps"). Freshness against the cache TTL | `c_macro_*`, `s_permission_leak` |
| 10 | `trigger` | GA | Issue created, updated and deleted; issue-link created and deleted; version events; project deleted; lifecycle installed and upgraded | Delivery is at least once, unordered, up to 3 min late, and some events never arrive (rob#21, #30, #33). Only top-level deletes are emitted (rob#36). The app receives its own events (rob#35). Moves change keys | `p_event_path`, `p_convergence`, `p_deletes_moves`, `p_self_events` |
| 11 | `consumer` (several queues) | GA | Signal drain, recompute, backfill chunks, migration, AI jobs | Concurrent by default (rob#28). Exact aggregates and latest-wins CI under concurrency. A throughput bar. `InvocationError` with `retryAfter` ≤ 900 s (rob#24). The cyclic limit of 1,000 (rob#18). `timeoutSeconds` up to 900, with the lowest timeout winning (rob#2, #3) | `p_concurrent_exact`, `p_throughput`, `p_continuation_exact`, `p_retry_semantics`, `p_poison` |
| 12 | `scheduledTrigger` (×3: `hour`, `fiveMinute`, `day`) | GA; ≤ 5 per app, one `fiveMinute` (tiers#39) | Hourly reconcile and backfill continuation; five-minute drain safety net and pause resume; daily privacy reporting | Duplicate runs, skipped ticks and overlapping runs (rob#40). A throw is not retried (rob#39). Spreading work over the hour against Tier 1 (tiers#23) | `p_schedules`, `q_bg_budget`, `q_privacy_pacing` |
| 13 | `webtrigger`, `response.type: static` | GA; static outputs [S]; HMAC tooling undocumented (sec#17) | **CI ingress**: signed build results | An app-implemented HMAC over the raw body with a timestamp (scheme stated, §E App. C). Atomic replay protection: `FAIL_IF_EXISTS` is not available in batches or transactions (rob#46), and an expired TTL value stays readable (rob#47). Bursts beyond 500 pushed events per minute (rob#15) force an inbox-and-drain design. The response must be static | `s_forged_ingress`, `s_webtrigger_scheme`, `s_replay_once`, `p_throughput` |
| 14 | KVS and Custom Entity Store | GA (rob#43–59) | CI signals and exceptions (entities with indexes), leases, nonces, budget ledger, policy, roles; secrets through `setSecret` | Conditional writes for exactly-once and lost-update safety. 10 KB unit accounting (rob#52). Query lag (rob#43). Cursors that cannot be persisted (rob#57). Index rules that are server-side lint: one `range` attribute, index-name rules, ≤ 20 entities (real-forge-fidelity §2.3) | `k_entities`, `p_concurrent_exact`, `q_platform_limits`, `e_kvs_units` |
| 15 | `sql` (Forge SQL) | GA [B]; `engine: mysql` [S] | **Portfolio store**: issues, releases, links, risk. Serves the Radar's list, sort, filter and paging | Migrations through `migrationRunner`, idempotent (platform §2.1). One statement per call. 150 DML requests per second; 4 MiB responses (rob#60). Parameterized queries. Adding SQL forces a major version [B] | `k_sql`, `s_egress_injection`, `q_platform_limits` |
| 16 | Realtime (`@forge/realtime` 1.0.1, bridge subscribe) | GA, CHANGE-3326 | Live Radar, panel and widget; AI results to the requester only | Token claims derived on the server (llm "newer facts" 5). Publish-only and subscribe-only tokens. 50 ops/s per installation, and `publish` returns `errors[]` without throwing (rob#63). `subscribe()` is scoped to the module context by default [B]; 1.0 defect D | `f_radar_live`, `s_realtime_isolation`, `q_platform_limits` |
| 17 | `llm` (`@forge/llm` 1.0.7) | GA [B]; one per app | **Explain risk** (per issue, person-facing) and **pre-generate** (batch per release) | A tool loop with parallel read-tool calls. Forced `report_risk` with validation, because there are no structured outputs (llm#10). Grounding. Permission-filtered prompts. Injection. Placement: a consumer with a timeout of at least 300 s, never a resolver (llm#23, #26). 100 requests per minute (llm#20). Model from `list()`. Sampling rules (llm#16, #17). Budgets, cache and cost | `m_*`, `s_prompt_injection` |
| 18 | Rovo `action` ×3 | GA [B] | `get-release-readiness` (GET), `get-issue-risk` (GET), `request-risk-exception` (CREATE) | Inputs are untrusted; identity comes from context (sec#25, #26). `actionVerb` must be truthful (llm#42). The CREATE action needs authorization and exactly-once | `r_*`, `s_rovo_inputs` |
| 19 | `rovo:skill` | **Preview**, CHANGE-3499 | `release-risk-analyst` skill over the two GET actions | Frontmatter rules: `name` equals the directory name, a lint ERROR in 1.0 (`m_skill_name`); `allowed-tools` ⊇ dependencies (llm#48, #49) | `k_rovo_wiring` |
| 20 | `rovo:agent` | GA (llm §8.1; page not re-fetched) | "Launch Control" agent listing the skill and the CREATE action | `name` ≤ 30 | `k_rovo_wiring` |
| 21 | `rovo:mcp` | **Preview** (external clients EAP: never demanded) | Exposes exactly the two GET actions | One per app; ≤ 50 tools; keys < 64 characters (llm#50) | `k_rovo_wiring` |
| 22 | `permissions.enforcement: app-managed` (rolling releases) | **Preview**, CHANGE-3315; key in schema [S] | Installations L and M run v2 code before the admins grant v2's new scopes | Degrade, then catch up exactly once after the grant. The upgrade event "is not triggered for code-only upgrades" (platform §2.7), so the migration cannot wait for it | `p_rolling_release` |
| 23 | Personal-data reporting | GA API (sec#30, #31) | Daily report of stored account ids; erase or refresh display names | Batches of 90 or fewer. The helper fires batches concurrently (sec#31), and the stated report API refuses concurrent requests | `s_privacy_erase`, `q_privacy_pacing` |
| 24 | Lifecycle events | GA (rob#37) | Install bootstrap (S, Z) and v1→v2 upgrade (L, M) | The installed event can arrive before the app's permissions, so the first asApp calls return 403 (rob#37) | `p_install_race`, `p_migration_*` |
| 25 | `invoke` `rateLimitProperties` | GA, CHANGE-3314 | Bootstrap under a 429 | Exactly one retry at or after `rateLimitReset` (stated rule; boot#16) | `b_429_boot` |

Module types in the manifest:
`jira:globalPage`, `jira:issuePanel`, `dashboards:widget`, `jira:adminPage`, `jira:projectSettingsPage`,
`jira:customField`, `jira:jqlFunction`, `jira:workflowValidator`, `macro`, `trigger`, `consumer`, `scheduledTrigger`,
`webtrigger`, `sql`, `llm`, `action`, `rovo:skill`, `rovo:agent` and `rovo:mcp`. That is **19 types**, against 10 in
1.0.

### B.2 Considered and excluded (no decorative modules)

| module | status | why it is out |
|---|---|---|
| `global:fullPage` | Preview | It would be a second front page next to the GA `jira:globalPage`. That is volume, not judgment. `jira:fullPage` is deprecated (CHANGE-3380); using it simply fails `k_modules` |
| `jira:command` | Preview | It can only open the Radar, so nothing would be graded beyond its presence |
| `jira:issueContext` | GA | `status` is static in the manifest schema [S], so it is decorative |
| `jira:workflowPostFunction` | Preview | It needs `manage:jira-configuration` (platform §2.11) only to duplicate what the trigger already does. Least privilege says no |
| `apiRoute` | Preview | The request object shape is undocumented, so it cannot be emulated faithfully (platform §2.6) |
| Object Store | Preview | It needs an emulated pre-signed host, and no product need justifies it |
| `rovo:agentConnector` | GA | It needs an external A2A agent, which is not offline-friendly |
| `dashboards:backgroundScript` | GA | No verified contract details in the brief |
| `jira:entityProperty` | (in schema [S]) | Allowed, as one valid way to back the JQL fragment. Not required |
| `dashboard:filters`, `global:ui`, external MCP clients, Containers, feature flags, `fifoConsumer`, user-based billing | EAP, unverified or unshipped | Never demanded (brief §10, §11.3) |

### B.3 Coherence: one fact, seven freshness mechanisms, five trust boundaries

This matrix is the heart of the angle. Every row is a surface the grader reads. Every column is a platform contract the
model must know (all are stated in the contract or documented). The cross-surface consistency row (`d_cross_surface`)
holds the surfaces to one oracle.

| surface showing "risk of issue X" | who sees it | platform contract that bites | how it stays fresh | what it may name |
|---|---|---|---|---|
| custom field (stored) | everyone who can view X | values shown on issue view and searchable; written only by the app | the app writes on change, so freshness is up to the app | the level only |
| `releaseRisk()` JQL | every searcher; Jira then filters each one's results | precomputation shared by all users, ≤ 1,000 values, 25 s | a field-backed fragment is automatic; an id list must be updated through the computation API | n/a (Jira filters) |
| Release gate | the transitioning user | synchronous; decides on state *now*; events lag ≤ 3 min | a fresh Jira read at transition time | counts only |
| issue panel | the viewer | person-facing; `extension` not validated | Realtime plus the store | the viewer's visible blockers, plus a hidden count |
| Release Radar | the viewer | person-facing; must stay useful during a quota wall | Realtime plus the store; bridge reads for visibility | visible items, plus hidden counts |
| dashboard widget | dashboard viewers | config per instance | Realtime plus the store | aggregates |
| Confluence static macro | every page viewer, even with no Jira access | cache key has no user; separate product storage; cross-product read | cache TTL ≤ the stated freshness | aggregates only |
| Rovo / MCP action | the invoking user | inputs untrusted; identity from context | computed on demand | visible items, plus hidden counts |

### B.4 Probe-gated items and the cut order (decided now, per 1.0 risk R3)

| order | item | gate (fidelity plan §I) | if the gate fails |
|---|---|---|---|
| cut 1 | app-managed permissions (`p_rolling_release`) | P26 measures how `appContext.permissions` is filled for a code-only upgrade with ungranted scopes (machinery N6: HIGH risk) | Drop the row. L and M get a plain major upgrade, with `avi:forge:upgraded:app` at T0 |
| cut 2 | Confluence static macro (`c_*` and the macro leak vector) | P20 checks the static-macro request and cache shape, a Confluence-invoked function calling Jira REST, and storage isolation between products | Drop C (0.02 is redistributed to D) and the macro sentences |
| cut 3 | Forge SQL (`k_sql` and the SQL parts of `q_platform_limits` and `s_egress_injection`) | P21: a TiDB-compatible local engine passes ≥ 95% of a conformance corpus run live and locally, and fits the packaging budget | The portfolio moves to CES aggregates. `k_sql` is replaced by an aggregate-correctness sub-check in `d_readiness` |
| cut 4 | workflow validator (`d_gate_decisions`, `k_validator_contract`) | P11 captures the function-form input, the error surface on the REST transition and the context user | Drop both rows. `d_cross_surface` loses the gate surface |

Every surviving row needs a live receipt or a doc quote that no live run contradicts (real-forge-fidelity §4, L1).

---------------------------------------------------------------------------------------------------------------------

## C. Architecture

Each subsection separates **what the contract prescribes** (WHAT) from **the reference design** (one valid HOW; the
golden). Grading checks outcomes, never code shape. The only prescribed storage choices are the three 1.0 already
prescribed by precedent: a named CES entity with its index, SQL for the portfolio, and the secret store for secrets.

### C.1 Data model

**Prescribed.**
- CES entity `ci-signal`, index `by-issue` (partition `issueId`, range `at`).
- CES entity `exception`, index `by-issue` (partition `issueId`, range `at`).
- Forge SQL holds the portfolio and serves the Radar's list queries.
- CI secrets live in the secret store (`kvs.setSecret`/`getSecret`).

**Reference design.**

| store | name | shape | why |
|---|---|---|---|
| CES | `ci-signal` | `issueId, runId, at (ISO), status, pipeline, deliveryId, origin (v1/ci)`; key `<issueId>:<runId>`; index `by-issue` [issueId] / [at] | Idempotent by business key, not `jobId` (rob#20). Latest-by-`at` is read through the index, not from delivery order |
| CES | `exception` | `issueId, requestId, requester, approver, status (pending/approved/rejected), reason, at, idem`; indexes `by-issue` [issueId]/[at] and `by-status` [status]/[at] | Exactly once through transaction `conditions` (rob#49); the approver queue is read through `by-status` |
| CES | `dead-letter` | `queue, itemId, reason, attempts, at`; index `by-queue` [queue]/[at] | Admin Jobs tab; poison items |
| CES | `budget-shard` | `hour, shard, points, version`; index `by-hour` [hour]/[shard] | A Tier 1 ledger that is safe under concurrent consumers: one shard per invocation lane, summed on read, conditional increments |
| KVS | `policy`, `pp:<projectId>`, `role:<accountId>`, `appr:<projectId>` | JSON | Configuration and roles (app feature; sec#9) |
| KVS | `pause:until`, `lease:<name>`, `wm:<kind>`, `drain:pending` | `FAIL_IF_EXISTS` with TTL, plus an `expireTime` check (rob#45, #47) | Pause-all; leases against duplicate or overlapping schedules; watermarks; the drain debounce |
| KVS | `nonce:<sha256(deliveryId)>` | `FAIL_IF_EXISTS`, TTL 25 h | Webhook replay protection for 24 h |
| KVS | `ai:cache:<hash>`, `ai:use:<yyyy-mm>`, `ai:user:<acct>:<day>` | JSON | AI cache keyed by installation, visibility scope, content hash and model; budgets |
| KVS secret | `ci-secret-1`, `ci-secret-2` | secret store | Rotation with two active secrets |
| SQL | `lc_issue(issue_id PK, project_id, issue_key, status_cat, level, blockers_open, ci_failing, late_scope, updated_at)` | Dates are strings [B] | Portfolio rows |
| SQL | `lc_issue_release(issue_id, release_id, added_at, PK(issue_id, release_id))`, `lc_link(blocker_id, blocked_id, PK)`, `lc_release(release_id PK, project_id, name, release_date, released, archived)`, `lc_meta(k PK, v)` | No foreign keys [B] | Aggregates by `GROUP BY`. Radar paging uses `LIMIT`/`OFFSET` or a keyset |

Each Jira installation (L, M, S, Z) has its own copy of all of this. Each Confluence installation keeps only its macro
state (§C.8).

### C.2 Async topology, continuation, scheduling

```
CI ─HMAC→ webtrigger ci.ingest ─(nonce FAIL_IF_EXISTS; inbox write)→ static 202 "accepted"
                       └─(drain:pending FAIL_IF_EXISTS, TTL 60 s)→ push {drain} delayInSeconds 30
                                                                     → q.signals (key "signals", limit 4)
Jira events → trigger events.* (classify; ids only; ≤ 50 per push) → q.recompute (key "rel:<releaseId mod 16>")
sched.hour (per-installation offset) → watermark reconcile + backfill cursor
         → q.backfill (key "jira-bg", limit 2)
    q.backfill: reserve points from the budget ledger → call → reconcile actual cost → checkpoint watermark →
                push next chunk. Quota pause or Retry-After > remaining time → InvocationError(retryAfter ≤ 900).
                Pause longer than 900 s → re-enqueue with delayInSeconds 900, hop by hop (scheduled-trigger safety net).
sched.fiveMinute → drain safety net; resume after pause; refresh JQL precomputations (id-list designs only)
code-only upgrade (no event!) → first sched.* run sees un-migrated v1 data → q.migrate (key "migrate", limit 1)
    migration: shard by v1 key prefix (no persisted cursor) → write v2 form → then delete the v1 key
panel / Radar / admin → q.ai (key "ai", limit 3, timeoutSeconds 600) → LLM tool loop
    → publishGlobal(channel, requester-claims token)
sched.day → privacy report: sequential batches of ≤ 90; honour Retry-After
```

Continuation rules the golden obeys. All of them come from documented limits.
- **Time.** Check `getAppContext().invocationRemainingTimeInMillis()` (rob#6) before each unit of work. Leave a stated
  margin. Checkpoint the watermark, then push a continuation.
- **Cyclic limit.** Chains descending from one origin stay under 1,000 push requests (rob#18). The hourly scheduler
  starts a fresh chain each hour, so a multi-hour backfill never hits the limit.
- **Push limits.** At most 50 events per push and 200 KB; at most 500 events per minute per installation (rob#15).
  Events carry ids only, which keeps them under 100 KB for long consumers (llm#64).
- **Concurrency.** Keys partition the work: release buckets, a single migration lane, AI lanes. Per-issue CI ordering
  is enforced by a conditional write on `at`, not by serializing everything.

### C.3 Resolver catalogue

Rules common to every resolver (all prescribed):
- Return a value and never throw; `{ "error": "<code>", "message": "…" }` on failure.
- Identity comes from `context.accountId` only.
- `context.extension` values are not trusted. The harness does not validate them (stated, brief §2.4 item 3).
- Refusal shape: `{ "error": "forbidden" }` with no state change.

| key | inputs | authorization (checked live) | Jira / Confluence calls (reference) | failure handling |
|---|---|---|---|---|
| `radar.bootstrap` | `{view?}` | any user who can load the page | none: SQL and KVS only (store-first). The client reads visible projects in parallel through bridge `requestJira GET /rest/api/3/project/search`, which costs 0 points (stated) | quota pause → returns `paused: true` with stored data |
| `radar.releases` | `{page, sort, dir, filter{projectId?, level?, text?}}` | release rows are filtered on the client by the visible-project set; the server returns no names for projects outside the `projectIds` the client sends **and** re-checks them with `permissions/check` | SQL with parameters; ≤ 25 rows per page | parameterized; invalid sort → `{error}` |
| `radar.release` | `{releaseId}` | the release's project must be browsable (live check) | SQL. Issue-level visibility comes from a client-side bridge JQL search (`fixVersion = …`) as the viewer | hidden = total − visible |
| `radar.recompute` | `{releaseId}` | `releaseManager`, or `approver` of the release's project | none; enqueue | coalesced: one job per release per 5 virtual min (stated) |
| `radar.exceptions` | `{page}` | approver of the project, or `releaseManager` | CES `by-status` | — |
| `panel.bootstrap` | `{}` (issue from context) | BROWSE on the context issue (asUser `GET /issue/{id}?fields=…` costs 2 points), because `extension.issue` can be forged | 1 GET; the store | not browsable → `forbidden` and nothing else |
| `panel.requestException` | `{issueId, reason, idem}` | BROWSE on the issue | CES transaction (conditional create) | exactly one pending request per (issue, requester) |
| `panel.decide` | `{issueId, requestId, decision, idem}` | approver of **that issue's** project (from a live read of the issue), or `releaseManager` | CES conditional update; ADF comment as the approver (asUser) carrying an `idem` marker; on 5xx or timeout, read the comments for the marker before any retry | exactly one exception decision and one comment |
| `ai.requestRationale` | `{issueId}` | BROWSE; AI budget; kill switch | none at request time; enqueue `{jobId, issueId, requester, visibleBlockerIds}` | cache hit returns at once; budget exhausted → stated error |
| `widget.view` | `{}` (release from `extension.config`) | the release's project must be browsable | SQL | no config → `needs-config` |
| `widget.options` | `{}` | — | bridge (client) or asUser project/version list | — |
| `admin.*` (bootstrap, savePolicy, setRole, listRoles, consumption, listFailed, retryFailed, discardFailed, generateSecret, rotateSecret, secretStatus, saveAi, migrationStatus) | per screen | **Jira ADMINISTER**, checked live (`permissions/check` with `globalPermissions: ["ADMINISTER"]`, brief §2.1). Verdicts reused for ≤ 60 virtual s | as needed | non-admin → `forbidden`, no diff |
| `admin.pregenerate` | `{releaseId}` | `releaseManager` or Jira admin | enqueue AI batch | budget-limited |
| `project.bootstrap`, `project.saveSettings` | `{projectId, overrides, approvers}` | **ADMINISTER_PROJECTS on `projectId`**, live. The project written must be the project checked | KVS | another project → `forbidden` |
| `gate.validate` (validator) | platform input `{issue:{key}, configuration, transition, modifiedFields}` (platform §2.11) | n/a (platform-invoked) | asApp `GET /issue/{key}?fields=status,issuelinks,fixVersions` (links carry linked-issue status); the store for CI, exceptions and policy | Jira error → `{result:false, errorMessage:"Launch Control: release risk data unavailable, try again"}` (stated) |
| `jql.releaseRisk` | `{precomputationId, clause}` (platform §2.12) | n/a; the answer must be user-independent | none (field-backed fragment); or SQL for ids ≤ 1,000 | invalid level → `{error}` |
| `macro.render` | `{macros:[{localId, context, config}]}` (platform §2.13) | n/a; aggregate-only output | cross-product Jira read asApp: `POST /search/approximate-count` ×3 per release | unknown release → a stated text |
| `rovo.getReleaseReadiness` / `rovo.getIssueRisk` / `rovo.requestException` | `versionId` / `issueKey` / `issueKey`, `reason` (string inputs; llm#63) | identity from context; the CREATE action as for `panel.requestException` | asUser search for the person's visible blockers (≤ 50); store totals | `{error}` without throwing; forged inputs ignored |
| `ci.ingest` (web trigger) | raw request (§E App. C) | HMAC with timestamp and delivery id | none | static outputs only |

### C.4 The Tier 1 dosing the app must implement (the stated rules and the reference algorithm)

**Stated physics** (contract §5 and Appendix B; every number is the benchmark's own where Atlassian publishes none).

- **Pool.**
  - 65,000 points per virtual UTC hour for the app across all installations (tiers#1).
  - A hard wall until the top of the next hour (tiers#7).
  - No carry-over (tiers#4).
  - No forgiveness (stated simplification, tiers#8).
- **Simulated co-tenants.**
  - The app is installed on many sites. The harness scores four of them.
  - The unscored rest spend 20,000 points per hour, spread evenly.
  - This is the real meaning of Tier 1 ("shared by every tenant", tiers#1, #27), and it makes overspend dangerous.
- **Fair share** (the benchmark's own policy, brief §6.2 demands one). Per scored installation per virtual UTC hour:
  - **background** work (scheduled, queue, trigger, web trigger, migration) spends at most **9,000 points**;
  - **all** Jira spend (background plus user-led) stays within **11,000 points**, so at least 2,000 points are always
    held back for people;
  - each **user-led** request (resolver, action, validator, JQL function, macro render) spends at most **100 points**.
- **The arithmetic that makes compliance safe** (stated): 20,000 + 4 × 11,000 = 64,000 ≤ 65,000. An app that obeys
  these rules can never wall the pool. The only wall is the injected co-tenant spike, so a non-injected wall is the
  app's own defect (`q_pool_exhausted`). The harness's user-led load is sized so that a correct app spends well under
  2,000 points per installation per hour (about 100 requests at ≤ 20 points typical).
- **Cost table** (Appendix B; tiers#5, #6, #30 corrected):
  - GET of a single core object: 1 + 1.
  - Identity or permission object: +2 each.
  - `search/jql`, `issue/bulkfetch`, `changelog/bulkfetch`: 1 + results.
  - `approximate-count`: 1.
  - `permissions/check`: 1 + 2 × (issues + projects checked).
  - Every write: 1, including a bulk custom-field update, whatever number of issues it covers.
  - Bridge `requestJira` from Custom UI: 0 points, uses burst buckets, receives headers (stated assumption, tiers#24,
    #25; FRGE-1923).
- **Wall scope.** The wall applies to calls charged points. Bridge calls are not walled.
- **Bursts.**
  - Per endpoint and method buckets: GET and POST refill 100/s, PUT and DELETE 50/s (tiers#10).
  - Capacity: twice the refill (stated, because it is unpublished).
  - Shared with simulated other-app traffic (CHANGE-2753).
  - Per-issue writes: 20 per 2 s and 100 per 30 s (tiers#13).
- **Headers** (tiers#14–19):
  - `RateLimit-Policy` and `RateLimit` lists in any order, with optional `r`.
  - `r` appears only past 80% of the pool.
  - A `Beta-` mix is possible.
  - `X-RateLimit-NearLimit` is optional legacy.
  - A gateway 429 can arrive without `Retry-After`; use `X-RateLimit-Reset`.
  - An unknown `RateLimit-Reason` is quota-class.

**What the app must do**, all graded on outcomes:
1. Keep its own ledger. The `r` value is invisible below 80% (tiers#17), and storage is per installation (tiers#35).
2. Pace background work to stay within its hourly share, spread across the hour rather than at :00 (tiers#23).
3. Still meet the completion bars:
   - L's backfill completes in ≤ 4 virtual hours. Its cost is about 20,300 points against a 9,000-point background
     share, of which L's event path uses about 1,200 points per hour. At about 7,800 points per hour the backfill
     takes about 2.6 hours, which gives the golden a 1.5× margin. N+1 reads (about 9 points per issue) take over 11
     hours.
   - S and M backfills complete in ≤ 1 virtual hour.
4. After any quota 429, make no backend Jira call from that installation before the reset. Deferred work then resumes.
5. On a burst 429, slow only that endpoint. On a per-issue 429, slow only that issue (tiers#20).
6. Retry only idempotent requests:
   - wait at least `Retry-After`, or until `X-RateLimit-Reset` when `Retry-After` is absent;
   - at most 4 attempts;
   - a write that answered 5xx is never retried blind. First check whether it landed.
7. Keep interactive surfaces useful during a wall. Store-first reads, plus bridge reads for the viewer's visibility,
   are the only design that does this.
8. Coalesce expensive user actions (the button-masher DoS, tiers#27). A recompute of the same release joins the running
   job for 5 virtual minutes.

**Reference algorithm (golden).**
- A `jira.js` wrapper reserves the estimated cost before each call against the budget ledger (CES `budget-shard`), then
  reconciles with the real result count.
- When the reservation would exceed this lane's share of the hour, it returns `defer`.
- A slice cap of one sixth of the hourly share per 10 virtual minutes smooths spend across the hour.
- The header parser feeds `pause:until` and the admin Consumption tab.
- User-led paths never call Jira for data the store already has. Visibility comes from the bridge (Radar, panel) or
  from asUser searches capped at 50 results (Rovo).

### C.5 Forge LLM features

**Explain risk** (person-facing, from the panel):
1. The resolver checks BROWSE, the AI budget and the kill switch. It looks up the cache, keyed by installation,
   visibility-scope hash (the sorted ids of the requester's visible blockers), content hash and model.
2. On a miss it enqueues ids only and returns `{jobId}` plus a **subscribe-only** Realtime token whose claims hold the
   requester's `accountId` (CHANGE-3326).
3. The `q.ai` consumer (`timeoutSeconds` 600) builds the prompt **only** from facts the requester can see. Hidden
   blockers appear as a count.
4. It calls `chat()` with three tools:
   - `get_issue_facts` (read-only);
   - `get_ci_history` (read-only);
   - `report_risk {summary: string ≤ 600, factors: [{factor: "BLOCKED"|"CI_FAILING"|"LATE_SCOPE", note: string}],
     citedIssueKeys: string[]}`.
5. The loop answers every tool call with a `role: "tool"` message carrying its `tool_call_id` (llm#9). Parallel calls
   are answered together. The loop is bounded at **4 model calls**.
6. The final turn is forced with `tool_choice: {type: "function", function: {name: "report_risk"}}`. The contract
   states that this forces the call, because Atlassian does not (llm#4).
7. Arguments are validated. Any other tool name is never executed.
8. Grounding:
   - the summary may contain only numbers the app computed for this issue, otherwise the app's own sentence replaces
     it;
   - cited keys are filtered to the requester's visible set;
   - the output is rendered as text.
9. The result is stored in the cache and published with `publishGlobal` to the requester's channel, using a
   publish-only token with the same claims. The panel shows it with an AI label.

**Pre-generate** (batch, release managers): 300 HIGH+ issues in a release.
- **Public-facts prompts only**: counts and own-project data, no blocker keys. The result can be cached for every
  viewer who can browse the issue.
- Paced at ≤ 100 requests per minute per installation (llm#20).
- Must finish within 15 virtual minutes. About 900 requests at 100 RPM take at least 9 minutes.

**Model choice.** Any id that `list()` reports `active` at call time. The emulator lists `claude-sonnet-4-5-20250929`
as `deprecated`, which is the real Anthropic status (llm "newer facts" 1). The contract says `list()` is the truth. No
sampling parameters that the documented rules reject (llm#16, #17; a deliberate knowledge trap, as in 1.0).

**Failures**, each with a stated shape and UI state (Appendix D):
- refusal (no tool call);
- malformed arguments;
- 403 `FORGE_LLMS_MODEL_FORBIDDEN` (llm#38);
- 429, where the SDK carries no `Retry-After`, so the stated rule is: no LLM request from that installation for 60
  virtual s, ≤ 3 retries per job, then "AI is busy";
- 500;
- a stream that ends without a non-null `finish_reason` (llm#11);
- an empty `end_turn`.

**Budgets and cost.** An installation monthly credit budget and a per-user daily budget are set in Admin. When either
is exhausted the app makes 0 calls and shows a stated message. Cost shown = emulator usage × stated credit rates:
- Haiku 10 / Sonnet 30 / Opus 50 credits per 1M tokens (llm#30);
- unpublished models are mapped by tier, labelled as an assumption (llm#32);
- $0.10 per input credit and $0.50 per output credit (llm#31).

**Hygiene.** No prompt, output or issue text in logs (llm#43). No LLM call during page boot or in any resolver.

### C.6 Front-facing Custom UI surfaces and their boot budgets

Three Custom UI surfaces, built by the entrant with the installed esbuild. Each may be its own resource or an `entry` of
a shared resource (multi-entry is GA, CHANGE-3337). The harness serves both forms; P17b settles bridge injection into
named entries before freeze.

| surface | READY means (stated selector plus seeded content) | SHELL | backend waves to READY | backend-bound ops before READY | app-origin bytes (gzip -9) | requests | initiator depth | bootstrap rounds / scaling | bootstrap payload |
|---|---|---|---|---|---|---|---|---|---|
| Radar | `[data-testid="release-row"]` × page 1, numbers equal to the oracle | header + `[data-testid="radar-shell"]` | ≤ 1 | ≤ 2 | ≤ 180 KB | ≤ 6 | ≤ 3 | ≤ 3; calls(4N) ≤ calls(N) + pages | ≤ 48 KB |
| panel | `[data-testid="risk-level"]` + factor counts + CI rows | `[data-testid="panel-shell"]` | ≤ 1 | ≤ 2 | ≤ 120 KB | ≤ 5 | ≤ 3 | ≤ 3 | ≤ 16 KB |
| widget view | release numbers | `[data-testid="widget-shell"]` | ≤ 1 | ≤ 1 | ≤ 100 KB | ≤ 5 | ≤ 3 | ≤ 2 | ≤ 8 KB |

These are provisional numbers (bootspeed §3.2). They are calibrated on the golden and the alt app, then **frozen as
numbers in the contract**. Calibration exits:
- gate-10 compliant: the golden's measured value × 1.5, then rounded;
- calibration rule: a frozen number never cuts into the golden.

The measurement protocol is "hold-and-release waves" (boot §4.3), cold, under production CSP:
- Host-local operations are free: `getContext`, `theme.enable`, `createHistory`, flags.
- Platform scripts and token CSS are excluded.
- READY is verified by seeded content, never by a marker.
- The scoring seed differs from the dev seed.
- The real `@forge/bridge` 7.1.0 runs, limiter included.

Also graded, counting not timing:
- zero repeated bootstrap invokes on in-session navigation (B13);
- zero LLM calls during boot;
- the shell renders while flags and Realtime subscriptions are held (B12);
- under a 429 on the bootstrap invoke the shell stays, with exactly one retry at or after `rateLimitReset` (B14);
- zero CSP violations and zero non-app, non-platform origins (B11).

Visual rules carried from 1.0 §7, all stated:
- `view.theme.enable()`, `--ds-*` tokens, 4.5:1 contrast in light and dark;
- a painted `--ds-surface*` background;
- a clean console;
- the widget works at 380 px with no clipped numbers.

### C.7 The UI Kit 2 admin panel and the project-settings page

Requirements stated in the contract (§8):
- `@forge/react` 12.3.0, React 18.3.1, `render: native`.
- The resource is a file, unless `bundler: manual@2026` is stated (uikit#19).
- Navigation uses **Tabs**, or the **Router** (Preview, supported on admin pages, uikit §1.4). Sidebar subpages do not
  exist for UI Kit (uikit#20).
- The host renders the stated component allowlist. Anything else is loud "unmodelled" (uikit §5.0 condition 6).
- No pixels are graded.

| tab (`Tab` testId) | screens and actions | resolver keys | server-side authorization | UI Kit judgment the grader observes |
|---|---|---|---|---|
| Policies `tab-policies` | weights (`Range`), thresholds and freeze days (`Textfield type="number"`), ready % ; Save | `admin.savePolicy` | Jira ADMINISTER, live | Invalid input (thresholds not strictly increasing) shows the stated `ErrorMessage` text and **0 invokes**. Valid input sends exactly one invoke with the stated payload. A double-activated Save sends one invoke. `Form.onSubmit` gets no data, so `handleSubmit` is required (uikit#28) |
| Roles `tab-roles` | DynamicTable of grants; `UserPicker` to add `releaseManager`; remove | `admin.setRole`, `admin.listRoles` | ADMINISTER; a user cannot change their own roles | UserPicker under `useForm` stores `{id,…}`, so the payload must carry `accountId: <id>` (uikit#29) |
| AI `tab-ai` | model (`Select` of `list()` active ids), installation monthly credits, per-user daily credits, kill switch (`Toggle`), usage this month | `admin.saveAi` | ADMINISTER | after exhaustion, 0 LLM calls plus the stated message |
| Consumption `tab-consumption` | points this hour (own ledger), pool `r` when present, paused-until, 429s by reason, LLM credits and $, KVS units, SQL DML peak | `admin.consumption` | ADMINISTER | values within the stated tolerance (±5%, or exact for counts) of emulator truth. Table sorted by numeric cell keys, never by display strings such as "$1,200" < "$900" (uikit#11) |
| Integration `tab-integration` | web trigger URL (`webTrigger.getUrl`); Generate secret (shown **once** in a `Modal`, `new-secret` testId on `Text`); Rotate (old secret valid for 15 virtual min); status "configured on …" | `admin.generateSecret`, `admin.rotateSecret`, `admin.secretStatus` | ADMINISTER | the secret is never returned again |
| Jobs `tab-jobs` | backfill progress per phase, migration "N of M", failed items (DynamicTable: Failed at, Queue, Reason) with Retry and Discard | `admin.listFailed`, `admin.retryFailed`, `admin.discardFailed`, `admin.migrationStatus` | ADMINISTER | the Failed-at column sorts by instant: the cell key is ISO, the display is human |

Project settings (`jira:projectSettingsPage`, UI Kit):
- Freeze days and HIGH/CRITICAL thresholds for this project.
- Approvers (`UserPicker isMulti`).
- Save calls `project.saveSettings`.
- Authorization: ADMINISTER_PROJECTS on that project, live. Payload `projectId` and context project are both untrusted.

UI Kit efficiency, stated:
- ≤ 2 invokes before the first ForgeDoc that contains the Policies form;
- the first ForgeDoc is non-empty (a loading state);
- ≤ 4 reconcile commits per action at quiescence;
- no `BridgeAPIError`.

### C.8 Web trigger, integrations, Confluence

**CI web trigger.** Static response type, outputs as stated in Appendix C:
- `accepted` 202
- `duplicate` 200
- `unauthorized` 401
- `invalid` 400
- `retry` 503

The app verifies, in order:
1. headers, case-insensitive and multi-valued (rob#42);
2. a timestamp within ±600 s of the harness clock;
3. an HMAC-SHA256 over `<timestamp>.<raw body>` with either active secret;
4. the delivery id, stored once for 24 h with `FAIL_IF_EXISTS`;
5. only then the inbox write.

Nothing happens before step 3 passes.

`hmacSharedSecret` is neither required nor penalised (sec#17). Header names follow RFC-141's body, pinned by the
contract: `x-webtrigger-signature: sha256=<hex>` and `x-webtrigger-timestamp`, plus our own `x-webtrigger-delivery`.

**Confluence** (probe-gated). The app is installed in Jira **and** in Confluence on L and Z. Its Confluence
installation has its own empty storage (sec §8). The static macro `release-status`:
- has a `releaseId` parameter;
- computes aggregates through a cross-product Jira read (asApp `approximate-count`, existence and behaviour by P20);
- returns one `renderedMacros[]` entry per requested `localId`, `contentType: xhtml` (platform §2.13);
- sets `cacheConfiguration.keyComposition` to include `macro.params`, so two releases on one page get separate cache
  entries;
- returns `cache.ttlSeconds` ≤ 900, because the stated freshness is ≤ 15 virtual min.

### C.9 Brownfield: v1 data, rolling release, migration

**The scenario.**
- L and M ran **v1 of Launch Control**. Their storage holds v1 records in a documented format (Appendix A):
  - plain KVS keys `ci:<issueId>:<runId>` → `{status, at, pipeline}`;
  - `exc:<issueId>` → `{approvedBy, at, reason}`;
  - `role:<accountId>` → `["releaseManager"]`;
  - `policy` → the v1 shape.
- v1 held the scopes `read:jira-work` and `storage:app`.
- The workspace starts **empty**, as an honest starter (1.0 §2.6). Only the sites carry v1 data.

**Why migration matters.** CI history and exceptions exist **only** in app storage, so a backfill from Jira cannot
recover them (stated).

**Rolling release** (probe-gated, cut 1):
- The harness rolls v2 code out to L and M as a **code-only upgrade**, which sends no upgraded event (platform §2.7).
- It grants v2's additional scopes later, at a virtual time the app cannot predict. That grant sends
  `avi:forge:upgraded:app` with `permissions` (platform §2.7).
- Until the grant, calls that need ungranted scopes fail with 403. The stated degraded behaviour:
  - approvals are recorded, and the comment is deferred, shown as `[data-testid="comment-pending"]`;
  - every deferred comment is posted exactly once after the grant.

**Migration guarantees:**
- every v1 record exists in v2 form;
- none is counted twice while live signals flow, deduplicated by `(issueId, runId)`;
- no v1 key is deleted before its v2 form is written;
- the migration survives a deadline kill.

Cursors are valid only within the invocation that produced them (stated, rob#57). A resumable scan therefore needs
prefix sharding, or migrate-then-delete. Persisting a cursor fails.

### C.10 Scopes and egress

- Least privilege: per call, the OpenAPI scopes (l_scopes rule from 1.0).
- The JQL-precomputation and app-field-value endpoints list only Beta-optional scopes `read:/write:app-data:jira` [O].
  Those are neither required nor counted as extra.
- No `permissions.external`: no egress.
- The web trigger is static.
- No remotes or providers.

Whole-app Runs on Atlassian eligibility is **not** graded, because SQL's residency status (`dareCompliant` [S]) is
unverified. The graded rule is "no egress, static web trigger".

---------------------------------------------------------------------------------------------------------------------

## D. The world

### D.1 Installations and scale

| world | seed | installations (Jira; Confluence where marked) | scale | role |
|---|---|---|---|---|
| **dev** | `dev_seed` | **D1** (v1 data, rolling release; + Confluence), **D2** (fresh install) | D1: 1,200 issues / 5 projects / 12 releases / 600 Blocks links; D2: 300 issues | Every graded behaviour occurs at least once (§D.5). Small enough for full runs in minutes |
| **W0** (the run's `fixture_seed`) | scoring | **L** (v1 → v2; + Confluence), **M** (v1 → v2), **S** (fresh install, install race), **Z** (security and UI bed; + Confluence), plus the **simulated unscored installations** (pool load only) | L: 10,000 issues / 18 projects / 100 releases (40 active) / 5,000 Blocks links (35% cross-project) / 2 projects with issue security levels / 40 users. M: 2,500 issues. S: 500. Z: 300 | All rows. The scale and dosing rows (`p_backfill_large`, `p_throughput`, Q, `m_dosing`) are W0-only |
| **W1, W2** (derived seeds, never `dev_seed`) | scoring | one installation each (fresh install, then the full timeline compressed into 2 virtual hours) | about 2,000 issues each | Correctness, security, UI, AI and Rovo rows. **Per-seed worst** with W0 |

Users per scored installation:
- 2 Jira admins;
- project admins;
- approvers;
- release managers;
- plain viewers;
- **two restricted viewers** who cannot browse two projects and one security level;
- one closed account for privacy.

At least 1,001 issues on L sit at HIGH or above. That makes the JQL cap of 1,000 right-hand-side values bite.

### D.2 Timeline (virtual time; W0)

```
T0       code rollout: L, M upgraded from v1 (code-only); S installed (asApp 403s for ≤ 10 min); Z installed
T0–T4h30 live load: issue updates (L 600/h, M 150/h, S 50/h, Z 30/h) with duplicates, reordering, ≤ 3-min delays, drops;
         CI steady 10 requests/min on L and M; user-led sessions (Radar, panel, widget, JQL searches as different users,
         transitions through the gate, approvals, Rovo, AI requests); world changes (§D.4); platform faults (§D.4)
T+40m    v2 scopes granted on L, M (upgraded event with permissions)          [probe-gated: rolling release]
T+2h10   CI release train on L: 3,000 signals in 120 virtual s, one request per run, hot set skewed
T+3h20   simulated co-tenant spike drives the pool to the wall until T+4h00 (top of the virtual hour)
C1..C6   checkpoints at T+30m, T+1h15, T+2h00 (UI phase 1), T+2h45, T+3h40 (during wall / after), T+4h30 (UI phase 2, final)
         — every checkpoint follows ≥ 10 virtual min with no relevant change (the convergence window)
```

**Grading start per installation.** This is stated, so it is fair to an app that spreads its backfill as the budget
demands.
- An installation's whole population is graded only from the first checkpoint after its backfill deadline:
  - S, M and Z: 1 virtual hour, so from C2;
  - L: 4 virtual hours, so at C6.
- Before that deadline, only issues that changed after install or upgrade are graded, within the convergence window.
  That includes issues touched by events, CI signals, links, moves or policy.
- So an event, or the release-train burst, on an issue that has not been backfilled yet must still produce that issue's
  full, correct risk.
- Migrated v1 records (CI history, exceptions) are graded from C2 on L and M. Their migration costs KVS units only, no
  Jira points, but must be paced under the limit of 4,000 write units per minute.

### D.3 Concurrency (stated physics)

- Without a concurrency key, the harness runs up to **25 deliveries of a queue at once per installation**. Physical
  parallelism, deterministic interleaving.
- With `concurrency {key, limit}`, at most `limit` deliveries sharing `key` run at once across all queues. Keys are
  installation-scoped (rob#28). Limit ≤ 50; ≤ 1,000 keys.
- Web-trigger requests and user-led resolvers run concurrently.
- Warm processes are reused across installations. Pending timers freeze at handler return, then resume in a later
  invocation, possibly another tenant's, or never (rob#12).

### D.4 Faults and mid-run changes, expressed as guarantees

The contract states **classes of world behaviour and platform semantics**, with their documented basis. It never states
timing, counts or targets. That is the line between 1.0 (whose fault list frontier models transcribed) and 2.0.

| class (stated in the contract) | documented basis | what the grader does (hidden: when, how many, on which objects) |
|---|---|---|
| product events: at least once, any order, ≤ 3 min late, some never | rob#21, #30, #33 | duplicates, permutations, delays, drops; app-caused events are re-emitted (`selfGenerated`) |
| Jira changes: issues move between projects and releases, are deleted, gain and lose Blocks links; releases are released, renamed, merged, deleted; projects are deleted (only that delete event is sent); permissions and policies change; accounts close | Jira behaviour; rob#36; P08 receipts | seeded sequences inside the windows between checkpoints |
| Jira's three limit systems, gateway 429s without `Retry-After`, 503 with `Retry-After`, a write applied but answered 5xx | tiers#9, #14, #15; tiers note §3.4 | quota wall (co-tenant spike), burst 429s with other-app bucket load, per-issue 429s, gateway 429, 503, 5xx-after-commit on comment POST |
| queue: redelivery after any failure, deadline kills (partial writes persist), retention 24 h | rob#21–#26 | kills at `timeoutSeconds`, redelivery after a kill that followed a side effect |
| storage: index queries may miss writes from the last 5 virtual s; batches fail per key; cursors valid within one invocation; expired TTL values stay readable | rob#43, #47, #51, #57 | query lag, partial batch failure, cursor invalidation, expired-but-readable nonces and leases |
| schedules: a tick may run twice, be skipped, or start while the previous one is still running | rob#40 (staff); stated emulator semantics | duplicates, skips, overlaps |
| install: the first asApp calls may return 403 for up to 10 virtual min | rob#37 | 403 window on S and Z |
| upgrade: code may run before new scopes are granted; the upgrade event arrives with the grant | platform §2.7 | rolling release (probe-gated) |
| process reuse: warm processes serve several installations; unawaited work may run later or never | rob#11, #12 | warm reuse, timer freeze |
| CI: resends, reorders, bursts up to 3,000 signals in 2 min, invalid and forged requests | the stated CI contract | malformed, unsigned, tampered, stale, case-varied, replayed sequentially and concurrently, rotated secret |
| LLM: the model may refuse, return malformed arguments, call tools in parallel, follow instructions found in issue text, be forbidden, rate-limited, fail, or stream an incomplete answer | llm#8, #11, #38; stated shapes | the scripted responder cycles every class |
| other people: users double-click, keep a second tab open, mash expensive buttons, try other people's resources | sec#4–#8 | the security battery (§F, S rows) |

**The guarantee sentence that governs all of it** (contract §4):

> At every checkpoint, after 10 virtual minutes without a relevant change, every value the app stores or shows equals
> the value computed from Jira's current state, the CI signals the app accepted, the exceptions decided and the current
> policy, whatever sequence of the behaviours above occurred before.

### D.5 Fairness invariant for the dev world (WP1 must prove it, as 1.0 §3 rule 3)

Each of the following occurs at least once in the dev world. The `forge-dev` tool exposes it (§H).
- every event type the harness emits;
- a move, a delete, a project delete;
- a release change;
- a link added and removed;
- a permission flip;
- a policy change;
- a closed account;
- a CI burst of 600 signals, which trips the 500-per-minute push limit for a push-per-signal design;
- every malformed-webhook class;
- every LLM answer class;
- every Jira 429 kind, a 503, a 5xx-after-commit;
- query lag, a partial batch, an invalidated cursor;
- a duplicate, skipped and overlapping schedule;
- the 403 install window;
- a code-only upgrade, then a grant;
- a warm-reuse timer freeze;
- an (on-demand) quota wall from co-tenant load;
- more than 100 results to page through;
- a security-level issue;
- a hidden cross-project blocker.

---------------------------------------------------------------------------------------------------------------------

## E. Contract outline

### E.1 The public input files and their size budget

| file | KB (est.) | contents |
|---|---|---|
| `spec-build-forge2.md` (prompt) | 3.6 | goal, budget sentence, definition of done (12 items), score bands in words, handoff |
| `FORGE2-CONTRACT.md` | 22.0 | §0–§12 below |
| `FORGE2-CONTRACT-APPENDIX.md` | 5.0 | A: v1 storage and scopes; B: points table, buckets, header grammar; C: CI web-trigger scheme; D: LLM error shapes and credit rates |
| `STARTER.md` | 4.4 | workspace, pinned packages (adds `@forge/react` 12.3.0, `@forge/sql` 4.0.7), reference material (schema, OpenAPI jira, jsw, conf, confv2), the `forge-dev` commands |
| total | **≈ 35 KB** | 1.85× 1.0's 18.9 KB, for 1.9× the module types and about 3× the graded behaviours. The contract opens with a 1 KB index so models can read it in pieces. Owner decision K6 |

### E.2 Contract sections and their key guarantee sentences (draft wording)

**§0 What it is, how it is graded** (0.9 KB).

> "The harness installs your app on several seeded Jira Cloud sites (and Confluence where stated), runs it for several
> virtual hours while the sites change, and grades what it stores, shows and does. Nothing is deployed. Time is virtual:
> limits, retries and windows are measured on the harness clock, never on wall time."

**§1 The risk model** (2.6 KB). Objects:
- a **release** is an unreleased, unarchived version that has a `releaseDate`;
- an issue is in a release when the release is among its `fixVersions`.

Factors:
- **BLOCKED** = count of distinct issues that block it through a `Blocks` link and whose status category is not Done, in
  any project;
- **CI** = the accepted signal with the greatest `at` (ties: greatest `runId`) has `status: failed`;
- **LATE** = the last time a release was added to its `fixVersions` is strictly after that release's freeze instant
  (`releaseDate` 00:00 UTC minus the project's freeze days).

Then:
- score = `wb·min(BLOCKED,3) + wc·CI + wl·LATE`;
- level by thresholds (defaults: weights 2/3/2, thresholds 2/4/7);
- issues in a Done category are LOW;
- readiness % rounded half away from zero to one decimal;
- "ready" = no not-done HIGH or CRITICAL issue without an approved exception, and done % ≥ the policy's ready %.

> "Values converge within 10 virtual minutes of the last relevant change."

**§2 Modules** (2.2 KB). The 19-row table: key, role, one sentence of WHAT. Plus:

> "Exactly one `jira:globalPage`, one `llm`, one `rovo:mcp`."
> "Your app makes no external network calls; its web trigger is static."

The scope rule is carried over from 1.0.

**§3 Storage** (1.6 KB): the prescribed entities and indexes, the SQL portfolio and the secret store. Then:

> "Index queries may miss writes made in the last 5 virtual seconds; `get` is exact."
> "A query cursor is valid only within the invocation that produced it."
> "Your Confluence installation has its own storage; it cannot read your Jira installation's."

**§4 Background guarantees** (2.6 KB). The governing sentence from §D.4, the behaviour classes of the §D.4 table, and:

> "Without a concurrency key, deliveries of a queue run concurrently (up to 25 at once per installation)."
> "Each installation's first backfill completes within 1 virtual hour (S, M) or 4 virtual hours (L) of install or
> upgrade. Until that deadline the harness grades only issues that changed after install or upgrade; from it, all of
> them."
> "CI may deliver up to 3,000 signals in 2 virtual minutes to one installation, one request per run; every signal you
> answer `accepted` is applied exactly once and reflected everywhere within 10 virtual minutes of the last."
> "Each logical action — a CI delivery, an exception request, an approval decision, its comment — has exactly one
> effect, whatever retries, double submissions, second tabs, concurrent Rovo calls, or Jira writes that were applied but
> answered with an error occur."
> "L and M ran v1 (Appendix A). Every v1 record survives in v2 form, none is counted twice, and none is deleted before
> its v2 form is written."
> "A failing item lands in the Jobs list with its reason after at most 5 attempts; everything else completes."

**§5 Tier 1 and platform budgets** (2.8 KB): pool, wall, co-tenants, fair share, user-led cap, wall scope, bridge rule,
burst and per-issue limits, header grammar (Appendix B), and the platform limits with their numbers (invocations, bridge
limiter, async, KVS, SQL, Realtime, LLM). Key sentences:

> "Your app is also installed on other sites, which together spend 20,000 points of the pool in every hour. Per
> installation and virtual hour, background work spends at most 9,000 points and all Jira calls at most 11,000; a
> request a person triggers spends at most 100. Within these limits your app can never reach the wall."
> "Your app's own traffic must never drive the pool to the wall."
> "After any quota 429, make no backend Jira call from that installation before the reset."
> "Retry only requests that are safe to repeat, never sooner than `Retry-After` (or `X-RateLimit-Reset` when it is
> absent), at most 4 attempts."
> "During a wall the Radar and the issue panel stay useful: stored risk data, the viewer's visible items, and
> `[data-testid="paused"]` with the reset time."
> "A recompute of the same release requested within 5 virtual minutes joins the running one."

**§6 Security** (2.6 KB). Roles:
- Jira administrators (ADMINISTER, checked live) are app admins;
- `releaseManager` is granted by app admins;
- `approver` per project is granted by that project's administrators (ADMINISTER_PROJECTS).

The protected-key table, then:

> "Every resolver and action is callable directly by anyone who can load its module, with any payload; module-context
> `extension` values are not validated. Authorize from `context.accountId` and live checks against the resource you act
> on; refuse with `{ "error": "forbidden" }` and change nothing."
> "A person sees keys, summaries and statuses only of issues they can browse; hidden things are counted, never named. The
> custom field, the gate's message and the Confluence macro name no issue at all."
> "A permission verdict may be reused for at most 60 virtual seconds."

Also in this section:
- the web-trigger pointer to Appendix C;
- secrets: CI secrets in the secret store, shown once at generation, never returned, logged, published or sent to the
  LLM;
- warm processes serve several installations, so nothing tenant- or user-specific may stay in module scope;
- user and CI text and AI output render as text;
- personal data: display names are stored; a daily report; closed accounts erased, updated ones refreshed; the report
  endpoint answers 429 to a request that starts while another is in flight.

**§7 Front surfaces, Custom UI** (3.0 KB): the Radar, panel and widget hooks, the §C.6 budget table with frozen numbers
and the measurement definitions, and the visual rules.

**§8 Admin surfaces, UI Kit** (2.2 KB):
- the tab testIds and texts;
- form payloads, for example `admin.setRole {accountId, role, grant}`;
- validation texts;
- sort semantics ("sorted by underlying value");
- consumption tolerance;
- the host allowlist;
- "no pixels";
- the classic-pragma `.jsx` rule (uikit §5.0 condition 2).

**§9 AI** (2.0 KB): placement, tools and schemas, the loop bound, forcing, validation and grounding, failure states and
retry rule, budgets and kill switch, the cache rule, `list()` as truth, requester-only delivery, the label, logs.

**§10 Rovo** (1.1 KB): three actions with inputs, outputs, verbs and errors; skill rules; agent; MCP tool list.

**§11 Confluence** (0.8 KB):

> "The release-status macro's output is shown to every viewer of the page, including people without Jira access; it
> shows the release name, done %, counts by level and ready — nothing else. Two macros for two releases on one page each
> show their own release; a page never shows readiness older than 15 virtual minutes."

**§12 Harness physics and deviations** (2.2 KB):
- the virtual clock;
- the latency model (Jira 150 ms + 0.5 ms per object; KVS 20 ms, batch or transaction 30 ms; SQL 25 ms + 0.02 ms per
  row; push 15 ms; Realtime 10 ms; LLM 1.5 s + 15 ms per output token, ≤ 180 s per call; CPU uncounted);
- the concurrency model;
- warm reuse;
- the redelivery schedule (1, 2, 4, 8, then every 15 virtual min within 24 h, as in 1.0);
- trigger `filter.expression` is not evaluated;
- the harness attaches the gate to every Done-category transition;
- seeds and scales differ from the dev site.

**Appendices** (5.0 KB): as in E.1.

### E.3 What the contract deliberately does not say

- When, how often or where any fault fires.
- Which issues are hot, moved or deleted.
- The economy optima beyond the stated E formulas (§F.5).
- Any HOW that the schema, typings, OpenAPI or dev site teach: module shapes, SDK calls, endpoint choices, bulk
  endpoints, entity-property or field-backed JQL fragments, bridge versus resolver reads.

---------------------------------------------------------------------------------------------------------------------

## F. Check registry

### F.1 Composition (1.0's, kept for one severity model across families)

`earned = (0.88·inner + 0.12·gate·e_mean) × crit_mult`; `final = min(earned, ceiling − 0.05·(1 − earned))`.

- `inner` is the weighted sum of the tier means below.
- Unavailable rows are excluded and make the verdict unpublishable.
- Vacuous rows (precondition unmet) price 0 and carry no multiplier.
- Every verdict reports inner, crit_mult and the unsuppressed criticals.

| L lint | K contracts | D data | P pipeline | Q tier/limits | S security | F front UI | B boot | A admin UI Kit | M AI | R Rovo | C Confluence | V visual |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| .05 | .07 | .12 | .15 | .12 | .14 | .08 | .05 | .07 | .07 | .03 | .02 | .03 |

**Measurement codes**, all independent of load:
- `SL` static lint, manifest or tree (run twice, identical);
- `GW` gateway log, with virtual timestamps;
- `OR` oracle comparison at checkpoints;
- `VT` virtual-time arithmetic;
- `KV`/`SQ`/`RT`/`LL` platform logs for KVS, SQL, Realtime and LLM;
- `DOM@Q` DOM at quiescence (the wave scheduler is idle, never a wall-clock wait);
- `FD` UI Kit ForgeDoc tree plus the bridge-call log;
- `CDP` per-frame coverage and initiator counts;
- `LOG` app log and canary scan.

A click or render that does not complete inside the generous real-time watchdog is **harness evidence**. It makes the
row `unavailable`, never 0 (§17.8 A).

### F.2 Rows

C = critical. W = weight in `inner`. "Anchor" is the contract section the row grades.

**L — lint and deployability (0.05)**

| id | measured | anchor | W | C | how |
|---|---|---|---:|:-:|---|
| `l_deployable` | 0 errors from: the client lint (16 linters, pins equal CLI 14.1.0); the **measured server-rule pack** (one range attribute, index name 3–50 characters and allowed characters, ≤ 20 entities, `nodejs20.x`, … per P19); the pinned deprecated-runtimes flag; and deploy-readiness blockers (two `jira:globalPage`, a second `llm` or `rovo:mcp`, > 5 scheduled triggers or > 1 `fiveMinute`, a UI Kit resource that is a directory or `.html`) | prompt done-1 | .012 | C | SL ×2 |
| `l_bundles_load` | share of manifest functions that bundle and load under the wrapper | prompt done-1 | .012 | C | SL |
| `l_lint_warnings` | 1 − 0.2·distinct warnings | prompt done-1 | .006 | | SL |
| `l_scopes` | declared = required per call (OpenAPI classic, else the full granular set; Beta-optional app-data scopes neutral); −0.25 per miss or extra | §2 | .012 | | SL + GW |
| `l_no_egress` | no `permissions.external`, remotes or providers; static web trigger; 0 egress attempts in the proxy log | §2, §6 | .008 | | SL + GW |

**K — module contracts and platform currency (0.07)**

| id | measured | anchor | W | C | how |
|---|---|---|---:|:-:|---|
| `k_modules` | fraction of the 19 required module types present, with the role §2 states | §2 | .012 | | SL |
| `k_current_apis` | no `@forge/ui`, no `storage` from `@forge/api`, no `/rest/api/3/search`, runtime `nodejs22.x`/`24.x`, no `jira:fullPage`, `dashboardGadget` or `issueGlance`, `@forge/react` ≥ 10 with `render: native` | §2 | .008 | | SL + GW |
| `k_entities` | `ci-signal` and `exception` declared with the stated indexes, and read through them | §3 | .008 | | SL + KV |
| `k_sql` | `sql` module; DDL through `migrationRunner`; second deploy runs 0 errors (idempotent); one statement per call; ≥ 1 Radar list served by SELECT | §3 | .008 | | SQ |
| `k_jql_contract` | answers `{jql}` or `{error}` for valid, invalid and missing arguments within 25 virtual s | §2 | .006 | | GW/VT |
| `k_field_contract` | string, read-only; values only through `/app/field/value`; type-compatible values | §2 | .006 | | GW |
| `k_validator_contract` | returns `{result, errorMessage}` for every attempted transition, no throw or timeout (probe-gated) | §2 | .005 | | VT |
| `k_macro_contract` | one rendered macro per requested `localId`, valid `contentType`; `keyComposition` present (probe-gated) | §11 | .004 | | log |
| `k_widget_edit_bridge` | edit through `@forge/dashboards-bridge` (`updateConfig`/`onProductSave` observed) | §7 | .004 | | bridge log |
| `k_rovo_wiring` | skill frontmatter and body rules; `allowed-tools` ⊇ deps; agent lists skill and CREATE action; MCP = exactly the two GET actions, `name` ≤ 30; `actionVerb` truthful | §10 | .005 | | SL |
| `k_timeouts` | `timeoutSeconds` only on consumer or scheduled functions, never shared with a 55 s module (lowest wins); AI consumer ≥ 300 | §9, §12 | .004 | | SL |

**D — data correctness and cross-surface consistency (0.12)**

| id | measured | anchor | W | C | how |
|---|---|---|---:|:-:|---|
| `d_backfill_small` | S and M: stored levels equal the oracle within 1 virtual hour (fraction) | §4 | .015 | | OR |
| `d_risk_values` | custom-field value per issue equals the oracle at each checkpoint (fraction; per-seed worst) | §1, §2 | .025 | | OR (REST GET) |
| `d_readiness` | readiness numbers per release (Radar resolver, widget, Rovo) equal the oracle | §1 | .015 | | OR |
| `d_factors` | seeded factor cases (strict-after freeze, re-added release, cross-project and closed blockers, CI latest-by-`at` with ties) equal the oracle | §1 | .015 | | OR |
| `d_jql_results` | `issue in releaseRisk(L)` as users A (wide) and B (narrow), in both orders, before and after changes; includes the > 1,000-match level on L | §2 | .015 | | OR (search) |
| `d_gate_decisions` | allow or refuse matrix on fresh state (blocker just added or resolved, exception just approved, policy just changed); message names no issue (probe-gated) | §2, §6 | .015 | | OR |
| `d_cross_surface` | at C3 and C6, sampled (issue, release) pairs agree across field, JQL, gate, panel, Radar, widget, macro, Rovo | §1 | .020 | | OR |

**P — pipeline robustness (0.15)**

| id | measured | anchor | W | C | how |
|---|---|---|---:|:-:|---|
| `p_event_path` | live changes reach the store within 10 virtual min | §4 | .009 | | OR |
| `p_convergence` | after each world-change window, every stored value equals the oracle (fraction over checkpoints × objects) | §4 | .013 | | OR |
| `p_concurrent_exact` | aggregates and latest-CI values exact under concurrent same-issue and same-release deliveries (seeded hot set) | §4 | .014 | | OR |
| `p_throughput` | release-train burst: fraction of signals reflected within 10 virtual min of the last | §4 | .014 | | OR/VT |
| `p_backfill_large` | L: fraction of issues correct at T0 + 4 virtual h (continuous) | §4 | .014 | | OR |
| `p_continuation_exact` | no lost or double item across deadline kills, quota pauses and redeliveries (backfill, drain, migration) | §4 | .008 | | OR |
| `p_exactly_once` | for every logical action, ≥ 2 effects observed (comments, decisions, requests, signal applications) | §4 | .010 | **C** | site + KV |
| `p_retry_semantics` | upstream 429 or 503 in background: `InvocationError` with `retryAfter` ≥ the signal, or re-enqueue; ≤ 5 virtual s slept in-function per invocation; no throw for 429 | §5 | .008 | | GW/VT |
| `p_poison` | poison items dead-lettered with reason after ≤ 5 attempts; the rest complete | §4 | .007 | | KV + FD |
| `p_schedules` | duplicate concurrent runs, skipped ticks and overlaps cause no double processing and no gap | §4 | .008 | | OR |
| `p_install_race` | S and Z bootstrap completes despite the 403 window | §4 | .006 | | OR |
| `p_migration_complete` | fraction of v1 records present in v2 form at C2 | §4 | .010 | | KV/OR |
| `p_migration_safe` | no double count during live signals; no v1 key deleted before its v2 write; resumes after a kill | §4 | .008 | | KV |
| `p_self_events` | app-caused events lead to ≤ 1 follow-up write per real change; no feedback chain | §4 | .006 | | GW |
| `p_deletes_moves` | moved issues keyed by id (new key shown); deleted issues and projects leave no rows, aggregates or field-backed results after the next window | §4 | .007 | | OR |
| `p_unawaited` | no side effect lands after its invocation returned | §4, §12 | .002 | | timer-freeze log |
| `p_rolling_release` | pending comments shown before the grant; posted exactly once after it; no crash (probe-gated) | §4 | .006 | | site + DOM@Q |

**Q — Tier 1 and platform-limit dosing (0.12)**

| id | measured | anchor | W | C | how |
|---|---|---|---:|:-:|---|
| `q_pool_exhausted` | non-injected `jira-quota-global-based` 429s in nominal hours. Severity = 1 − walled virtual minutes / 60 | §5 | .012 | **C** | GW |
| `q_bg_budget` | mean over (installation, hour) of `min(1, 9000 / background points) · min(1, 11000 / all points)`. Continuous, no cliff | §5 | .018 | | GW |
| `q_user_led_cost` | fraction of user-led requests ≤ 100 points | §5 | .008 | | GW |
| `q_pause_all` | after a quota 429: backend calls before the reset (per installation; fraction compliant); deferred work resumes | §5 | .014 | | GW/VT |
| `q_burst_scoped` | after a burst 429 on endpoint E: E waits ≥ `Retry-After`, other endpoints continue | §5 | .010 | | GW |
| `q_per_issue` | per-issue windows respected; after `jira-per-issue-on-write`, that issue waits | §5 | .008 | | GW |
| `q_retry_discipline` | retries ≥ `Retry-After` (or `X-RateLimit-Reset`); ≤ 4 attempts; no blind retry of a non-idempotent write | §5 | .012 | | GW |
| `q_headers_truth` | Admin Consumption values (own ledger, `r`, paused-until, 429s by reason) against truth under header permutations | §5, §8 | .010 | | FD vs GW |
| `q_platform_limits` | fraction of limit classes never violated by the app's own pattern: push per minute and per request, cyclic, KVS RPS and units, SQL DML RPS and response size, Realtime ops, LLM RPM, invocations per user per minute, bridge 500 per 25 s | §5 | .014 | | KV/SQ/RT/LL/bridge |
| `q_button_masher` | 30 recompute clicks in 1 virtual min yield ≤ 1 job per release per 5 virtual min; points bounded | §5 | .007 | | GW |
| `q_privacy_pacing` | report batches ≤ 90; no overlapping requests; `Retry-After` honoured | §6 | .007 | | site |

**S — security (0.14)**

| id | measured | anchor | W | C | how |
|---|---|---|---:|:-:|---|
| `s_privilege_escalation` | every protected resolver or action, called directly by an unauthorized principal (non-admin, wrong-project admin, non-approver, payload `isAdmin`/`role`), **changed protected state** | §6 | .012 | **C** | KV/SQ/site diff |
| `s_refusals` | the same calls answered `{error:"forbidden"}` | §6 | .004 | | resolver log |
| `s_same_resource` | project-admin and approver checks bound to the acted-on project or issue (payload and context forgeries) | §6 | .010 | | diff |
| `s_payload_identity` | payload `accountId` ignored; attribution from context | §6 | .005 | | site/KV |
| `s_permission_leak` | a hidden canary (key, summary, description of an issue the viewer cannot browse; security-level issues) **observed** in any person-facing output: resolver responses, DOM, Realtime to that user, a person-facing LLM prompt, Rovo or MCP output, the macro, the gate message, the custom field | §6 | .014 | **C** | LOG/DOM/RT/LL scan |
| `s_hidden_counts` | hidden counts equal the oracle (panel, Radar, Rovo) | §6 | .007 | | OR |
| `s_permission_ttl` | after a permission flip, no stale verdict served after 60 virtual s | §6 | .007 | | OR/VT |
| `s_forged_ingress` | a web-trigger request failing authentication **produced a side effect** | §6, App. C | .010 | **C** | KV diff |
| `s_webtrigger_scheme` | stated output per malformed class; valid requests with raw-body variance (whitespace, key order) accepted; rotation grace honoured | App. C | .010 | | response log |
| `s_replay_once` | the same delivery sequentially and concurrently yields one effect; expired-but-readable nonces handled | App. C | .010 | | KV/OR |
| `s_secret_hygiene` | secret in the secret store; absent from responses after generation, logs, Realtime, prompts | §6 | .008 | | LOG scan |
| `s_tenant_leak` | an installation-A canary **observed** in installation-B output, storage or logs under warm reuse | §6 | .010 | **C** | scan |
| `s_xss` | no script execution (canary global), no `javascript:` links, from issue text, CI pipeline names or AI output | §6 | .006 | | DOM |
| `s_prompt_injection` | the scripted model "obeys": no write, no unlisted tool, no hidden key, no link | §9 | .008 | | site/LL/DOM |
| `s_rovo_inputs` | forged action inputs ignored; CREATE authorized from context; invisible keys refused | §10 | .005 | | action log |
| `s_realtime_isolation` | user B on A's channel receives nothing; shared-channel payloads carry ids only | §6, §7 | .006 | | RT |
| `s_privacy_erase` | closed account erased, updated account refreshed | §6 | .004 | | KV/DOM |
| `s_egress_injection` | path-manipulation payloads blocked (`route` throws, brief S22); SQL filter injection returns the literal-match results | §6 | .004 | | GW/SQ |

**F — front Custom UI function (0.08)**

| id | measured | anchor | W | C | how |
|---|---|---|---:|:-:|---|
| `f_radar_loads` | the Radar shows ≥ 1 release with numbers | §7 | .008 | **C** | DOM@Q |
| `f_radar_content` | list sort, filter and paging (≥ 2 pages on L); detail by level with visible blockers and hidden counts; exception queue; router links | §7 | .016 | | DOM@Q |
| `f_radar_live` | an open Radar shows new numbers within the window without reload; 0 invokes while idle; ≤ 1 refetch per 5 virtual s of updates | §7 | .010 | | DOM@Q + bridge log |
| `f_panel` | level, factors, visible blockers, CI history, exception state | §7 | .012 | | DOM@Q |
| `f_approval_flow` | approve: decision recorded, one ADF comment as the approver, success flag; non-approver sees no control | §7 | .010 | | site/DOM |
| `f_during_wall` | during the wall: Radar and panel show stored data, visible items and `paused` | §5 | .008 | | DOM@Q |
| `f_widget` | edit (project and release) through the edit API; view numbers; a second instance is independent | §7 | .008 | | DOM@Q |
| `f_ai_flow` | rationale delivered to the requester with the AI label; a stated state for each failure class | §9 | .008 | | DOM@Q + RT |

**B — boot, all counted (0.05)**

| id | measured | anchor | W | C | how |
|---|---|---|---:|:-:|---|
| `b_shell` | shell at wave 0 (Radar, panel, widget) | §7 | .006 | | CDP |
| `b_waves_ops` | ≤ 1 wave; ≤ the stated backend ops before READY | §7 | .010 | | wave log |
| `b_bytes_requests_depth` | app-origin gzip bytes, request count and initiator depth against the frozen numbers | §7 | .008 | | CDP |
| `b_bootstrap_scaling` | ≤ stated rounds; calls do not scale (seed N vs 4N); payload ≤ the stated KB | §7 | .008 | | proxy rounds |
| `b_session_reuse` | 0 repeated bootstrap invokes on navigation; 0 LLM calls during boot; flags and Realtime do not gate the shell | §7 | .006 | | bridge log |
| `b_429_boot` | a 429 on the bootstrap invoke: shell stays; exactly one retry at or after `rateLimitReset` | §7 | .005 | | bridge log/VT |
| `b_uikit_boot` | admin: ≤ 2 invokes before the first Policies ForgeDoc; non-empty first ForgeDoc; ≤ 4 commits per action | §8 | .007 | | FD |

**A — admin, UI Kit (0.07)**

| id | measured | anchor | W | C | how |
|---|---|---|---:|:-:|---|
| `a_admin_loads` | tabs with the stated testIds render | §8 | .006 | | FD |
| `a_policy_form` | invalid input gives the stated texts and 0 invokes; valid input gives exactly one invoke with the stated payload; double Save gives one invoke | §8 | .010 | | FD |
| `a_roles` | UserPicker grant carries `accountId`; remove works; self-change refused | §8 | .008 | | FD + KV |
| `a_tables` | header clicks sort by underlying values (instants, decimals, currency); paging | §8 | .008 | | FD |
| `a_consumption_truth` | Consumption tab values within tolerance of truth | §8 | .010 | | FD vs GW/LL/KV |
| `a_jobs` | failed items with reasons; Retry and Discard work | §8 | .007 | | FD + KV |
| `a_ai_budget` | budget and kill switch enforced (0 calls, stated message) | §8, §9 | .007 | | LL + DOM |
| `a_secret_rotation` | secret shown once; presence-only afterwards; rotation grace | §8 | .006 | | FD + response log |
| `a_project_settings` | project admin edits own project; values take effect within the window | §8 | .008 | | FD + OR |

**M — Forge LLM (0.07)**

| id | measured | anchor | W | C | how |
|---|---|---|---:|:-:|---|
| `m_model_current` | every request uses an id `list()` reports `active`; 0 validation 400s from sampling parameters | §9 | .008 | | LL |
| `m_tool_loop` | parallel tool calls answered with matching ids; ≤ 4 calls; forced final tool; unlisted tools never executed | §9 | .010 | | LL |
| `m_validation_grounding` | malformed arguments give an error and no side effect; digits and keys rule; cited keys filtered | §9 | .010 | | LL + DOM |
| `m_failure_states` | each failure class gives its stated state; non-AI features keep working; retry rule after 429 | §9 | .010 | | LL + DOM |
| `m_placement` | 0 LLM calls from resolvers or during boot; consumer timeout ≥ 300 s; ids-only events | §9 | .008 | | LL + SL |
| `m_dosing` | batch of 300 finishes in ≤ 15 virtual min with ≤ 100 RPM per installation in every 60 s window | §9 | .010 | | LL/VT |
| `m_cache` | identical repeat makes 0 new calls; unchanged rerun makes 0; no cross-visibility reuse | §9 | .008 | | LL |
| `m_hygiene` | AI label present; no prompt or output text in logs | §9 | .006 | | LOG/DOM |

**R — Rovo (0.03)**

| id | measured | anchor | W | C | how |
|---|---|---|---:|:-:|---|
| `r_action_results` | readiness and issue-risk JSON per person equal the oracle | §10 | .012 | | action log |
| `r_action_errors` | unknown or missing inputs give `{error}`, no throw | §10 | .006 | | action log |
| `r_exception_request` | CREATE: one pending request per (issue, requester); authorization; status values | §10 | .012 | | KV |

**C — Confluence, probe-gated (0.02)**

| id | measured | anchor | W | C | how |
|---|---|---|---:|:-:|---|
| `c_macro_values` | each macro shows its release's readiness, aggregate-only | §11 | .010 | | render log + OR |
| `c_macro_cache_freshness` | two macros for two releases on one page differ; readiness ≤ 15 virtual min old at checkpoints | §11 | .010 | | render log/VT |

**V — visual (0.03)**

| id | measured | anchor | W | C | how |
|---|---|---|---:|:-:|---|
| `v_theme_tokens` | `enableTheming`; text tokens; contrast ≥ 4.5 in both modes | §7 | .008 | | computed styles |
| `v_dark_mode` | own `--ds-surface*` paint; no blank surface | §7 | .008 | | pixels + tokens |
| `v_csp_console` | 0 CSP violations, 0 failed assets, 0 console errors in nominal flows | §7 | .008 | | CDP |
| `v_layout_380` | widget at 380 px: no horizontal overflow, numbers not clipped | §7 | .006 | | DOM |

Totals: 110 weighted rows; tier sums verified to 1.000.

### F.3 Criticals (floor 0.6, `factor = 0.6 + 0.4·severity_input`). Each fires only on its named defect, OBSERVED

| critical | consequence | severity input | what does NOT fire it (§17.8 B rule) |
|---|---|---|---|
| `l_deployable` | deploy blocked | 0 on any error | warnings; documented constraints the server does not enforce (v04–v06 in real-forge-fidelity §2.3) |
| `l_bundles_load` | crash | 0 if < 40% load, else 1 | — |
| `s_privilege_escalation` | a non-authorized principal changed protected state | cliff 0 | a wrong refusal shape with no state change (`s_refusals`) |
| `s_permission_leak` | a person saw something hidden | cliff 0 | a wrong hidden **count** (`s_hidden_counts`) |
| `s_forged_ingress` | forged CI input applied | cliff 0 | a wrong status code with no side effect (`s_webtrigger_scheme`) |
| `s_tenant_leak` | cross-customer leak | cliff 0 | — |
| `p_exactly_once` | duplicate side effect on a customer's Jira or records | cliff 0 | zero effects, a missing flag, a 404 (graded in `f_approval_flow` / `r_exception_request`). The duplicate target is always a fresh, viewer-visible object |
| `q_pool_exhausted` | the app walled every tenant | `1 − walled_minutes/60` | injected walls (co-tenant spike) |
| `f_radar_loads` | dead primary flow | 0 if no release renders | harness click or render timeouts (unavailable) |

**ROOT_BLOCKS** (attribution plus multiplier dedup, as in 1.0):
- `l_bundles_load` blocks every runtime row.
- `f_radar_loads` blocks the Radar rows of F, B and V.
- `s_permission_leak` blocks `s_hidden_counts` **only when both trace to the same read path**, as observed: identical
  over-broad response.
- `q_pool_exhausted` blocks `q_pause_all` in the hour it caused.

**Vacuity.** Every "no defect" row needs its surface exercised. Without that it is `vacuous_root`: 0 and no multiplier
(1.0 G5).

### F.4 Admission bands (pass = exactly 1.0, available, not vacuous)

| band | max | requires |
|---|---:|---|
| 1 deployable | 0.499 | `l_deployable`, `l_bundles_load` |
| 2 working core | 0.699 | `d_backfill_small`, `d_risk_values` ≥ 0.9 on S (the quiet-world subset), `p_event_path`, `f_radar_loads`, `a_admin_loads`, `k_entities` |
| 3 complete current platform | 0.799 | every surviving `k_*` row, `f_panel`, `f_widget`, `a_project_settings`, `r_action_results`, `m_model_current`, `v_theme_tokens`, `v_dark_mode` |
| 4 production robustness (graded) | `max(0.799, 0.899 − 0.03·(n−1))` | 57 rows: P (non-critical, except band-2 `p_event_path`) 15, Q (non-critical) 10, S (non-critical) 14, D-consistency (`d_cross_surface`, `d_jql_results`, `d_gate_decisions`) 3, M (except `m_model_current`) 7, B 7, `f_during_wall` 1; n counts **defects** after root dedup (1.0 `band_defects`) |

The prompt states these bands in words, with a parity test, as in 1.0.

### F.5 Excellence (0.12 slice; continuous, formulas stated in the contract so nothing is an unstated preference)

Gate: every K row = 1.0, `v_csp_console` = 1.0, `l_lint_warnings` = 1.0.

| id | ratio (continuous `min(1, top/ratio)`, `top` = golden's worst of 5 seeds) | stated formula |
|---|---|---|
| `e_points` (0.35) | total Jira points for the scenario ÷ reference | "the reference spends X points on W0's scenario; you earn this share by approaching it" (X published at freeze) |
| `e_kvs_units` (0.20) | 10 KB KVS units ÷ reference | as above |
| `e_llm_credits` (0.25) | LLM credits ÷ reference | as above |
| `e_invokes` (0.20) | frontend invokes in the scripted UI sessions ÷ reference | as above |

This answers the 1.0 lesson that `e_event_economy` sat at 0.40 for every frontier model because its optimum was never
stated (frontier §4). These targets are published as numbers.

### F.6 Scoring sequence (serial; one world at a time; never concurrently with another scoring)

0. Three worlds: W0 (run seed), W1, W2 (derived; never `dev_seed`). Each world gets a fresh site set, emulator and
   Chromium context. Correctness rows take each world's **worst**. E rows take the **mean**.
1. Clone the tree, then the kit's pristine modules: lint ×2, the server-rule pack and deploy-readiness; bundle and load.
2. Start the world:
   - Jira mocks for L, M, S, Z (plus Confluence on L and Z);
   - the points gateway with co-tenant load;
   - the warm multi-tenant runner;
   - the discrete-event scheduler.
3. Seed v1 data on L and M. Code-only rollout. Install S and Z.
4. Run the timeline of §D.2 in virtual time. At C1–C6 run the oracle comparisons: REST field reads, JQL as users,
   resolver and action reads, macro renders, store snapshots.
5. UI phase 1 (C3) and UI phase 2 (C6), in Playwright:
   - Radar, panel and widget, cold, with boot metrics, in light and dark, as wide and restricted viewers;
   - Admin and project settings in the UI Kit host;
   - flags are drawn outside the app frame (§17.8 A).
6. The security battery runs on Z and on W1/W2: direct resolver and action calls, web-trigger attacks, warm-reuse
   canaries, XSS, injection.
7. Composition, bands, report, media.

Estimated wall time:
- W0: 12–18 min, about 13,000 invocations through warm workers plus 2 UI phases;
- W1 and W2: 4–6 min each;
- **total 20–30 min per tree**.

This is an estimate, not a measurement. Freeze item 12 measures it.

### F.7 How the registry obeys the §17.8 rules

| rule | where |
|---|---|
| harness or probe failure is never app evidence | DOM@Q plus watchdog gives `unavailable`; the host draws flags outside the app frame |
| emulator UI never covers app controls | same |
| a critical fires only on the defect it names, observed | §F.3 column 4 |
| each root priced once | ROOT_BLOCKS plus band-defect dedup |
| no one-call cliffs | budgets and economy are continuous; budget compliance is `min(1, cap/spent)` |
| per-seed worst for correctness | §F.6 step 0 |
| one-defect mutants per check | §J.3 |
| an independent alt app scores ≥ 0.95 | §J.4 freeze item 7 |
| every schema-valid form either runs or is refused with the real CLI's message (defect C) | the emulator supports consumer `resolver:` and `function:` forms, the `crossVersion` key (no grading until P28), and trigger `payload.include` (P27) |
| contract text equals emulator behaviour verbatim (defect D) | Realtime pairing and claims copied from P13 receipts |

---------------------------------------------------------------------------------------------------------------------

## G. Difficulty argument

### G.1 What the frontier did on 1.0 (frontier-behaviour.md, quoted)

- "On everything Forge 1.0 states, the frontier has no headroom … Their entire residual … is `e_event_economy`."
- "The contract did the hard thinking. Every semantic trap that both models handled traces to a contract sentence that
  Haiku paraphrased."
- "All six defect classes Haiku found by testing were platform wiring, each announced by an explicit dev-tool error
  string."
- "The world is small and stays put … Consumers ran strictly one at a time … The longest Retry-After was 30 s."
- What would defeat them, ranked by the study: "concurrency with a throughput requirement, scale against real Forge
  limits, idempotency under ambiguous failures and a Retry-After longer than the invocation, a world that changes
  mid-run, a live migration of installed data, and a contract that states invariants instead of listing the faults."
- What would not: "Pure volume (more modules, surfaces, files)", "API and manifest trivia", "Unstated grader
  preferences".

This design uses every vector on the first list. It keeps breadth only where breadth forces a choice (§B.3), and it
states every economy target.

### G.2 What GPT-6.1 Sol and Opus 5.5 would most plausibly fail, and why

The Sol and Opus trees were not on this machine for 1.0 (frontier §0 caveat). The reasoning below extrapolates from
Haiku and Pareto, which sit 0.0108 and 0.0319 below them, and from the requirements' structure. Ranked by my confidence
that a frontier run loses real points there.

1. **Throughput under concurrency** (`p_throughput`, `p_concurrent_exact`). High.
   - Haiku C18: "I'm accepting the eventual consistency tradeoff here since concurrent writes both read fresh state
     anyway". Pareto serialized the whole site with `limit: 1`, "which is correct but has a throughput ceiling".
   - In 2.0 consumers are concurrent by default (a stated platform fact). The release train needs about 25 requests per
     second for 2 minutes.
   - Global serialization at about 0.4 virtual s per signal takes about 20 minutes and misses the 10-minute bar.
   - Unpartitioned concurrency loses aggregate updates and lets an older CI signal overwrite a newer one.
   - A push per signal hits 500 events per minute.
   - Only an inbox with a debounced drain, partitioned keys and conditional writes passes everything.
2. **Two-sided Tier 1 dosing** (`q_bg_budget`, `q_pool_exhausted`, `p_backfill_large`). High.
   - Haiku planned a watermark and shipped full rescans: "My plan: persist a `lastRunAt` watermark …", while the code
     shipped `Math.min(...startDates) - 2 * DAY`.
   - Here the L backfill (about 20,300 points) cannot fit one hour of the installation's 9,000-point background share
     (11,000 including user-led spend), yet it must finish within 4 virtual hours.
   - The app needs self-accounting that survives concurrent consumers. A read-modify-write budget counter undercounts
     exactly when the throughput work runs in parallel.
   - It also needs pacing across hours and a resumable cursor that is never persisted.
   - An app that ignores the budget loses `q_bg_budget` continuously, and risks the wall in hours where its
     installations' backfills coincide on top of the 20,000-point co-tenant load. An app that crawls misses the
     deadline.
3. **Exactly-once under ambiguous failure** (`p_exactly_once`, critical). Medium-high.
   - Pareto: "Never retry a successful POST; the UI coalesces a double click before invoking this resolver." Haiku:
     "rather than adding backend-side deduplication".
   - In 2.0 the approval comment POST is applied by Jira but answered with 5xx. A second tab and a concurrent Rovo
     CREATE target the same request.
   - UI-only deduplication posts a duplicate comment.
4. **The breadth-born conflicts** (`d_jql_results`, `c_macro_*`, `f_during_wall`, `d_gate_decisions`,
   `s_permission_leak`). Medium.
   - Each is stated. Each is plausible to miss in a 19-module build where attention is spread thin:
     - an id-list JQL fragment fails above 1,000 matches and goes stale without precomputation updates;
     - a JQL answer computed `asUser` shows one user's view to everyone;
     - a macro whose `keyComposition` lacks `macro.params` shows one release's readiness under another release's
       macro;
     - a macro that names blocker keys leaks them, a critical;
     - a gate that reads only the store lets through an issue whose blocker was added 2 virtual minutes ago;
     - a panel that reads visibility through backend `asUser` calls goes dark during the wall.
   - The frontier strength ("they transcribed the contract's trap list") does not cover this. The sentences state
     guarantees, and the mechanism must be derived from platform facts spread across §2, §5, §6 and §11.
5. **Live migration without a persisted cursor** (`p_migration_*`). Medium.
   - "Neither app versions its storage."
   - The v1 key format forces prefix sharding or migrate-then-delete. A code-only rollout sends no upgrade event, so a
     migration that waits for `avi:forge:upgraded:app` starts 40 virtual minutes late. CI history is missing until then,
     and C1 fails on L.
6. **UI Kit host semantics** (`a_policy_form`, `a_roles`, `a_tables`). Medium.
   - `Form.onSubmit` without data, the UserPicker object, and sort keys that are display strings are what the brief
     calls traps for strong models (uikit §5.2 traps 3–5).
   - 1.0 had no UI Kit, so no frontier behaviour on it has been observed. Medium-low confidence that it costs Sol much.
7. **The boot budget with a non-scaling bootstrap** (`b_waves_ops`, `b_bootstrap_scaling`). Medium-low.
   - Haiku and Pareto were never measured on this.
   - A `getContext → invoke(config) → invoke(data)` waterfall, or a bootstrap that reads per release, fails. A frontier
     model that reads the budget table may well pass.

### G.3 Predicted scores and spread

Expected inner score by tier for Sol:

| tier | L | K | D | P | Q | S | F | B | A | M | R | C | V |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Sol | .95 | .85 | .65 | .50 | .55 | .80 | .75 | .55 | .65 | .70 | .90 | .60 | .95 |

Composition:
- Weighted inner ≈ 0.69.
- Excellence: gate fraction about 0.5, `e_mean` about 0.6, worth about +0.036.
- Pre-critical earned ≈ 0.64.
- Probability that at least one critical fires, my estimate: `p_exactly_once` about 35%, `s_permission_leak` about 30%
  (macro, gate, Realtime and prompt vectors), `q_pool_exhausted` about 15%. Expected multiplier about 0.82.

| entrant | predicted final | why |
|---|---|---|
| reference (golden) | 1.000 | freeze gate |
| GPT-6.1 Sol / Opus 5.5 | **0.40–0.70**, point estimate about 0.55. Two criticals would take it to about 0.25 | loses on P, Q, D-consistency, B; possibly one critical |
| Haiku 5.5-class | 0.25–0.50 | breadth plus testing load; UI Kit; dosing |
| mid open models | 0.10–0.35 | band 2 or 3 caps; partial surfaces |
| weak or broken | 0.00–0.10 | band 1 (lint), or vacuity |

This separates weak, mid and frontier models without 0 for all but the top and without 0.9 for everyone (constraint
8). There is headroom for future models, so no 2.1 is needed soon (owner).

### G.4 Why this difficulty is engineering judgment, not trivia or volume

- **Trivia is cheap by design.** Module shapes, SDK calls and endpoints are all offline (schema, typings, OpenAPI, dev
  site). L plus K weigh 0.12, and a frontier model gets them about right.
- **Volume is bounded.**
  - The reference app is about 10,500 LOC.
  - A frontier model writes that in 30–60 calls: Pareto wrote 11 files in 11 calls, and Haiku wrote 5 backend files in
    one response.
  - The rest of the budget goes to the hard behaviours, all reproducible on the dev world.
- **Every heavy row is a stated guarantee with competing constraints.** Cost against freshness (JQL, macro, gate).
  Security against Tier 1 (the permission TTL, bridge versus resolver). Throughput against correctness (concurrency).
  Availability against points (the wall). Migration against live traffic.
  - None can be passed by paraphrasing a sentence. Each needs a design that holds under all its constraints at once.
  - The contract never says which design. The dev world shows every behaviour, but only at small scale.

### G.5 How the claim gets proved before money is spent (NOW.md phases 3, 4 and 7)

**Phase 4, pre-build difficulty proof.**
- Three fresh Opus-class planning agents get only the public files and write a full design plus test plan.
- Graders check the plans against the hidden registry.
- Expected misses, as in §G.2: at least 3 of {inbox drain, budget ledger concurrency, cursorless migration, JQL > 1,000,
  macro cache key, wall-time UI, ambiguous-5xx comment}.
- If the plans miss fewer than 2, harden first. Hardening levers in order: raise the burst and scale within platform
  limits, tighten the freshness window, add world-change kinds. Never add trivia.

**Phase 7, the pilot.** "Challenged" means:
- Sol final ≤ 0.80;
- ≥ 3 band-4 defects on stated rows;
- every lost row backed by app evidence (no `unavailable`, no harness-attributed loss);
- the golden at 1.000 on the same kit.

---------------------------------------------------------------------------------------------------------------------

## H. Fairness argument: a capable model can know every graded thing

1. **Stated.** Every number and hook a row measures is in the contract or its appendix:
   - formulas, windows, budgets, cost table, header grammar, scheme, schemas, testIds, texts, payloads, boot numbers,
     credit rates, latency model.
   - A test (`test_score_forge2.py`, as 1.0 §14) fails the build when a row has no anchor or an anchor has no row.
2. **Platform facts not in offline material are stated as facts, not as faults.** For example:
   - JQL precomputations are shared across users, capped at 1,000 values, 25 s;
   - macro cache keys carry no user;
   - storage is per product;
   - async semantics;
   - the upgrade event's absence on code-only upgrades;
   - LLM limits and the forcing meaning of `tool_choice`.

   Each cites a verified source in the design. The contract text carries the fact.
3. **Discoverable offline.**
   - Schema 13.6.0: module keys, `macro.static`, `consumer.crossVersion`, `permissions.enforcement`.
   - Pinned typings (`@forge/*`, including `@forge/react` 12.3.0 and `@forge/sql` 4.0.7).
   - The Jira OpenAPI: `/app/field/value` with `generateAppEvents`, the precomputation endpoints, `permissions/check`,
     bulk endpoints.
   - The Confluence OpenAPI.
   - The lint messages.
4. **The dev world shows everything once** (§D.5). The kit adds dev tools on top of 1.0's set:
   - `forge-dev quota`: the points ledger per installation and hour, with `r` and NearLimit as the gateway emits them;
   - `forge-dev world`: replay the dev change script, or one change kind;
   - `forge-dev ci --burst N` and `--attack <class>`, with a dev secret helper that signs;
   - `forge-dev upgrade`: v1 data, then code-only rollout, then grant;
   - `forge-dev uikit serve`: the admin and project-settings pages in the UI Kit host, with the ForgeDoc dump and the
     bridge log;
   - `forge-dev macro render`;
   - `forge-dev gate <issue> <transition> --as <user>`;
   - `forge-dev jql "<jql>" --as <user>`;
   - `forge-dev llm`: cycle the scripted answers;
   - `forge-dev wall`: a co-tenant spike on demand.
5. **Scale is stated, not hidden.** The contract gives the scoring scales: issues per installation, link counts, the
   burst size, more than 1,000 HIGH+ on one installation. The dev world is small, so the model must *reason* about
   scale. That is constraint 7 of the frontier study in a fair form: no error is hidden, only scale differs, and the
   scale is stated.
6. **Preview features are demanded as documented on a date.** The pinned schema and typings plus the contract text are
   the reference. There is no internet.
7. **No unstated preference.** Economy targets are published (§F.5). Any design that reaches the outcome passes: grading
   is on outcomes, never on module kind (frontier §5.4: Pareto's scheduled → queue → consumer design must not lose).

---------------------------------------------------------------------------------------------------------------------

## I. Fidelity plan

### I.1 Layers (real-forge-fidelity §4, adopted)

| layer | what | when |
|---|---|---|
| L0 | offline lint = CLI client half + pinned deprecated-runtime flag `["sandbox","nodejs18.x","nodejs20.x"]` + **measured server-rule pack** + CLI-equal file walk (`.mjs`, `.cjs`, UI Kit resource directories) | every score; corpus re-run at freeze plus a weekly drift job |
| L1 | the emulator held to real Forge by a **differential conformance suite**: the same probe source live and emulated; any difference fails the kit test; a row depends only on receipted behaviour | before each freeze |
| L2 | real deploy: (a) golden plus platform-semantics mutants deploy, install and smoke-test on wolfaenpak; (b) an audit lane for our own baseline runs (deploy only) | freeze; per baseline run |
| L3 | browser calibrations (boot ranking golden vs slow mutant; Realtime delivery; UI Kit callbacks) in a quiet window | before freeze, never during scoring |

### I.2 Probes (all on wolfaenpak, the sanctioned test site; throwaway apps; restored afterwards)

| id | behaviour (rows that depend on it) | probe sketch (same source live and emulated) | status |
|---|---|---|---|
| P01 | KVS/CES codes, atomicity, limits (`k_entities`, `s_replay_once`, `p_*`) | 24-case probe | **done**: 13/24 identical; align the 11 diffs (409 `KEY_CONFLICT`, 400 `CONDITIONAL_CHECK_FAILED`, 422 for a 26-op transaction, …) |
| P02 | CES query: range conditions, sort, `limit` 101, TTL expiry lag (`p_migration_*`, `s_replay_once`) | seed N rows, query; TTL 60 s read at +30/+65/+120 s | planned (5 min) |
| P03 | CES optimistic concurrency under parallel consumers; KVS rate-limit codes (`p_concurrent_exact`, `q_platform_limits`) | 50 events, transaction with `check` on one key; a write burst | planned (10 min) |
| P04 | queue retry schedule; `retryReason` values (`FUNCTION_TIME_OUT` vs the emulator's `FUNCTION_ERROR`); `retryAfter` clamp; `retryData` 4 KB (`p_retry_semantics`) | a consumer throws or returns `InvocationError`; time it with `forge logs` | planned (40 min idle) |
| P05 | delivery parallelism with and without keys; duplicates; ordering; push limits 51/201 KB/500 per minute codes (`p_throughput`, `q_platform_limits`) | push 50 with and without `concurrency`; sleep 10 s; overlap | planned (10 min) |
| P06 | timeouts per module; what the web-trigger caller sees (`k_timeouts`, `p_continuation_exact`) | functions that log each second | planned (15 min) |
| P08 | product events for: issue moved (key and project items), issue deleted, project deleted (no child events), link created/deleted, version released/renamed/merged/deleted, fix-version changelog, `selfGenerated` with and without `ignoreSelf`; event names and payloads (`p_convergence`, `p_deletes_moves`, `p_self_events`) | a trigger app logging events while a REST script performs each change | planned (1 h) |
| P09 | stored custom field: `POST /app/field/value` limits (ids per entry), changelog and events, `cf[]` search, type errors (`d_risk_values`, `k_field_contract`) | write 1,001 / 5,000 ids; search | planned (20 min) |
| P10 | JQL function: input shape, context identity, precomputation sharing across two users, update API, 1,000-value error, CHANGE-3506 400, timeout caching (`d_jql_results`) | function returns id lists and field fragments; two users search | planned (40 min) |
| P11 | validator function form: input, context user, how `errorMessage` reaches a REST transition (400 body), time limit (`d_gate_decisions`) | attach to a wolfaenpak workflow; transition via REST | planned (40 min), **gate for cut 4** |
| P12 | web trigger: request object (`body, call, context, contextToken, headers, method, path, queryParameters, userPath`, observed), static outputs, raw body bytes, multi-value headers, `getUrl` GraphQL (N7), 429 headers (`s_webtrigger_*`) | probe router keyed on `userPath` | partly done (request shape, 424 after uninstall) |
| P13 | Realtime: `publish`/`subscribe` pairing, context-scoped vs global, token claims, publish-only/subscribe-only, 50 ops/s errors (`f_*_live`, `s_realtime_isolation`) | Custom UI page plus consumer | planned (1 h, browser leg in L3) |
| P14 | Forge LLM: validation errors, `list()` statuses, stream chunking with tool calls, RPM 429 shape, `finish_reason` values, usage (`m_*`) | small prompts; cents of tokens | planned (30 min) |
| P15 | UI Kit admin page: callback argument shapes (`Form`, `Tabs`, `UserPicker isMulti`, `Textfield` types), ForgeDoc capture, DynamicTable sorting, Router `createHistory` (`a_*`, `b_uikit_boot`) | probe admin page driven by forge-live-harness | planned (2 h, L3) |
| P16 | project settings page context and resolver context (`a_project_settings`, `s_same_resource`) | as P15 | planned |
| P17 | Custom UI: issue panel and global page contexts; bridge `requestJira` header forwarding (FRGE-1923); `rateLimitProperties` field names; **P17b**: bridge injection into a named `entry` | probe pages | planned (1 h, L3) |
| P19 | server-rule lint corpus extended to every 2.0 module (`macro.static`, `jqlFunction`, `customField`, validator, native admin and project-settings, static web trigger, `sql`, `rovo:*`) plus `forge eligibility` | `scripts/lint_corpus.py` | planned (≈ 40 manifests, 5 min) |
| P20 | static macro request and response, batching and concurrency, cache key and TTL, unknown `localId` fallback; **Confluence-installed function calling Jira REST**; **storage isolation between products** (`c_*`) | a dual-product throwaway app on wolfaenpak Jira and Confluence | planned (1.5 h), **gate for cut 2** |
| P21 | Forge SQL conformance: DDL and DML, dates as strings, single-statement rule, policy violations, FK refusal, response cap, `migrationRunner` idempotency (`k_sql`) | 40-statement corpus live vs local engine | planned (1 h plus engine spike), **gate for cut 3** |
| P22 | upgrade event payload on a major upgrade (`forge install --upgrade`) | lifecycle trigger logging | planned |
| P24 | privacy API responses with the documented test ids (`s_privacy_erase`) | report Active and Closed ids | planned (5 min) |
| P25 | `permissions/check` asApp with `accountId`; `ADMINISTER` global; `ADMINISTER_PROJECTS` (`s_*`) | resolver probe | planned (10 min) |
| P26 | app-managed permissions: `appContext.permissions` on a code-only upgrade with ungranted scopes; `hasPermission` shape; the grant's upgrade event (`p_rolling_release`) | `forge install --upgrade code --major-version` (platform §2.7) | planned (1–2 h), **gate for cut 1** |
| P27 | trigger `payload.include.fields`/`propertyPaths` delivered shape | trigger app | planned (15 min) |
| P28 | `consumer.crossVersion` [S] semantics with events queued across a deploy | push, deploy v+1, observe | planned (20 min) |

Total live work: about 2–3 agent-days to build the probe app family, then about 4–6 hours of mostly idle wall time per
calibration pass. Browser legs run in quiet windows only.

### I.3 What stays unmeasured, and how the design avoids grading the ambiguous side

| unmeasured | why | how it is handled |
|---|---|---|
| hourly quota, point costs, forgiveness, bucket capacity | docs forbid rate-limit testing on cloud tenants (tiers#48) | stated benchmark physics; rows grade compliance with the **stated** physics only; no claim of production equivalence |
| scheduled duplicates, skips, overlap rates | not reproducible on demand | documented possibility; stated emulator semantics; any lease or watermark design passes |
| event delay and drop distributions | platform-internal | stated classes; convergence graded only after settle windows |
| real model text and moderation shapes | scripted model | stated shapes; model quality never graded |
| asApp visibility of security-level issues | undocumented (sec#10 outdated) | stated worst case: asApp sees everything |
| `context.extension` validation | docs conflict (sec#1) | stated: not validated. A correct app checks live permissions either way |
| who can open admin pages | ECO-1592; undocumented | not graded; authorization is graded in resolvers |
| wall-clock boot, cold start | load-sensitive | not graded; counts only; L3 confirms the ranking |
| timing-safe HMAC comparison | not observable from outside | not stated, not graded |
| cross-product macro reads (if P20 fails) | — | cut 2 |
| `crossVersion` delivery | unmeasured until P28 | no row depends on cross-version events |

---------------------------------------------------------------------------------------------------------------------

## J. Build plan, effort, risks, sizes, budget, cost, time, packaging

### J.1 Work packages (one owner per file; 2.0 lives in its own tree, integration D1)

| WP | owns | contents | agent-days |
|---|---|---|---:|
| WP0 probes | `forge2/probes/**` | P01–P28 apps and scripts, the conformance-suite fixtures, the server-rule corpus | 2.5 |
| WP1 kit core | `forge2/kit/**` | in-child virtual-time agent (timers and `Date` over IPC; fixes N1); discrete-event scheduler (proxy as choke point; fixes N4 and N11); warm multi-tenant runner with timer freeze; product-plural proxy (Confluence, fixes N2; GraphQL `getUrl`, fixes N7); KVS aligned to P01–P03 (codes, units, lag, cursor invalidation, TTL lag); async (keys, limits, jobs, retries); lint (server pack, flag, walk); `forge-dev` v2 | 5 |
| WP2 site and world | `forge2/site/**` | mutable world with timeline (routes the 31 direct `pack.*` reads through state); indexed fixtures at 10k issues (fixes N3); JQL (`cf[]`, function substitution, precomputation cache, 1,000 cap); stored custom field and app-field API; workflow and validator callback; permission model (security levels, project roles, `ADMINISTER`/`ADMINISTER_PROJECTS`, `permissions/check`); points gateway (cost table, buckets, per-issue limits, wall, co-tenants, headers, faults); privacy mock; LLM responder v2 (tool loops, parallel calls, RPM windows, stream truncation, deprecation); Realtime claims; Confluence page renderer and macro cache; SQL engine bridge | 6 |
| WP3 UI hosts | `forge2/kit/lib/{uikit-host,bridge-host}*` | UI Kit host (real reconciler and bridge; ForgeDoc → accessible HTML; serialiser; ADS comparator; P15 fixtures); Custom UI host extensions (global page and issue panel contexts, flags outside the frame, wave scheduler, CDP coverage and initiators) | 3.5 |
| WP4 scorer, probe, oracle | `bench/score_forge2.py`, `forge2_probe.mjs`, `forge2_oracle.py`, thresholds, controls, tests | 110-row registry, composition, bands, selftest, ROOT_BLOCKS, oracle (indexed, per checkpoint), scoring sequence, evidence diet (page summaries, NDJSON spill; fixes N9), media | 5 |
| WP5 golden | `bench/golden-forge2/**` | built from public material only (1.0 §13.3 discipline) | 4 |
| WP6 alt app | `bench/golden-forge2-alt/**` | independent: field-backed vs entity-property JQL; bridge vs asUser visibility; Router vs Tabs; `stream()` vs `chat()`; sharded vs conditional ledger | 3.5 |
| WP7 mutants | `forge2/mutants/**` | ≥ 1 per weighted row (about 110), anchor-asserting patches | 2.5 |
| WP8 integration | tier `FORGE20`, per-version release manifest, desktop per-era tables, site per-era snapshots (integration §3–§7) | 2 |

Total about 34 agent-days. With 4–5 parallel packages that is **about 8–10 calendar days**, plus probe wall time. Order:
1. WP0 and the P01/P19 alignment first.
2. WP1 and WP2 in parallel; WP4 against hand-made worlds; WP5 on `forge-dev` as soon as WP1 lands.
3. WP3.
4. WP6 and WP7.
5. Freeze.

### J.2 Reference app size (estimate)

| part | LOC |
|---|---:|
| manifest | 350 |
| Jira client (cost accounting, buckets, retries, headers) and budget ledger | 900 |
| risk model, store (CES, KVS, SQL, migrations) | 1,000 |
| pipeline (triggers, drains, backfill continuation, reconcile, schedules) | 1,200 |
| web trigger (HMAC, replay, inbox) | 250 |
| JQL, field sync, gate, macro | 700 |
| resolvers and authorization (roles, permission TTL cache keyed by installation and user) | 1,300 |
| AI (tool loop, validation, cache, budgets, Realtime) | 600 |
| Rovo, privacy, migration, Realtime helpers | 650 |
| Custom UI: Radar, panel, widget (view and edit), shared | 2,300 |
| UI Kit: admin, project settings | 1,300 |
| total | **≈ 10,550 LOC, about 65 files** |

That is about 5× 1.0's golden (about 2.1k).

### J.3 One-defect mutants (examples; at least one per weighted row)

- `m_global_serial` → `p_throughput`
- `m_racy_aggregate` → `p_concurrent_exact`
- `m_push_per_signal` → `q_platform_limits`, `p_throughput`
- `m_no_budget` → `q_bg_budget` (plus `q_pool_exhausted` in the co-tenant hour)
- `m_rmw_budget_counter` → `q_bg_budget`
- `m_sleep_retry_after` → `p_retry_semantics`
- `m_persist_cursor` → `p_migration_complete`
- `m_wait_upgrade_event` → `p_migration_complete` at C1
- `m_delete_then_write` → `p_migration_safe`
- `m_jql_idlist` → `d_jql_results` (> 1,000)
- `m_jql_asuser` → `d_jql_results`
- `m_macro_key_content_only` → `c_macro_cache_freshness`
- `m_macro_names_keys` → `s_permission_leak`
- `m_gate_from_store` → `d_gate_decisions`
- `m_gate_names_blockers` → `s_permission_leak`
- `m_panel_trusts_extension` → `s_permission_leak`
- `m_resolver_visibility_in_wall` → `f_during_wall`
- `m_comment_blind_retry` → `p_exactly_once`
- `m_ui_only_dedupe` → `p_exactly_once`
- `m_hmac_reserialized` → `s_webtrigger_scheme`
- `m_nonce_get_then_set` → `s_replay_once` (concurrent)
- `m_ttl_trusted` → `s_replay_once`
- `m_secret_in_status` → `s_secret_hygiene`
- `m_module_cache_by_issue` → `s_tenant_leak`
- `m_ai_in_resolver` → `m_placement`
- `m_answer_first_tool_only` → `m_tool_loop`
- `m_llm_digits` → `m_validation_grounding`
- `m_temp_and_top_p` → `m_model_current`
- `m_client_claims` → `s_realtime_isolation`
- `m_userpicker_object` → `a_roles`
- `m_sort_display_key` → `a_tables`
- `m_onsubmit_expects_data` → `a_policy_form`
- `m_boot_waterfall` → `b_waves_ops`
- `m_render_after_invoke` → `b_shell`
- `m_privacy_helper_concurrent` → `q_privacy_pacing`
- `m_no_ignoreself` → `p_self_events`
- `m_key_not_id` → `p_deletes_moves`
- `m_project_admin_any_project` → `s_privilege_escalation`

### J.4 Freeze gate (all must hold)

1. The golden scores 1.000 on W0, W1, W2 and 5 calibration seeds; every critical is exactly 1.0; no `unavailable` and
   no `harness_missing`; every contract hook produces non-vacuous evidence.
2. The extended severity selftest passes:
   - a leak scores below a missing surface;
   - a duplicate below a missing approval;
   - pool exhaustion below a missing widget;
   - the empty starter and a one-function app score ≤ 0.05;
   - the band-4 graded ceilings are exact.
3. Lint double runs are identical. Kit lint equals the real CLI on the P19 corpus.
4. Thresholds frozen with receipts (budgets, boot numbers, E tops), sha-pinned. The golden's margin is ≥ 1.5× on every
   time or budget bar, against the virtual latency model.
5. Each mutant loses exactly its rows. Every **platform-semantics** mutant also misbehaves on real Forge (L2a).
6. The P01–P28 receipts for surviving rows replay identically in the emulator.
7. The alt app scores ≥ 0.95 with zero `harness_missing`.
8. L2 real-Forge golden gate: deploy, install (Jira and Confluence) and smoke-test on wolfaenpak.
9. Cross-model sanity review: one cheap and one mid model run, every fired critical reviewed by hand, no row dominated by
   harness evidence (the §17.8 lesson).
10. Golden-vs-golden spread under load measured for any ratio row.
11. Entrant path: one cheap run from the Benchmark view (gate 3).
12. Scoring time measured: ≤ 35 min per tree on the workhorse.

### J.5 Risks, ranked by my confidence that the design holds as written (not by effort)

| # | risk | confidence it holds | mitigation |
|---|---|---|---|
| R1 | **Deterministic concurrency scheduler**: new engineering with no precedent in the repo (machinery 4.2: MEDIUM-LOW). CPU-bound code between proxy calls is invisible | MEDIUM-LOW | proxy choke point plus in-child timer agent; a real-time watchdog makes the result `unavailable`, never 0; K schedule seeds keep the worst; P03 and P05 check that races reproduce on real Forge |
| R2 | **Emulator surface breadth**: more mocked behaviour means more chances of a §17.8-class harness defect deciding ranks | MEDIUM | conformance suite (L1); alt app; cross-model sanity review; flags outside the frame; cut order decided in advance |
| R3 | **Forge SQL engine**: fidelity and packaging (TiDB-compatible; size unmeasured) | MEDIUM-LOW | probe-gated (cut 3) with a complete CES fallback |
| R4 | **Confluence cross-product calls** unverified | LOW-MEDIUM | probe-gated (cut 2) |
| R5 | **Too hard for the budget**: frontier < 0.3 | MEDIUM that the frontier lands at 0.40–0.70 | 250-call budget; bands and partial credit; Phase 4 plan check; cut by fidelity order, never the pipeline |
| R6 | **Contract size (≈ 35 KB) invites a desk audit** | MEDIUM | the index, tables, appendices; Phase 4 measures planning calls |
| R7 | **UI Kit host callbacks** inferred | MEDIUM | P15 capture on wolfaenpak; allowlist; loud "unmodelled" |
| R8 | **Scoring time** grows beyond 30 min with deterministic scheduling and three worlds | MEDIUM | warm runner; evidence diet; W1 and W2 compressed; measure at freeze |
| R9 | **Calendar**: about 34 agent-days | MEDIUM-LOW that it lands in under 2 weeks | parallel WPs; cut order |
| R10 | **Bimodal scores** from 9 cliff criticals | MEDIUM | the criticals are production-severity only; one is continuous (`q_pool_exhausted`); owner decision K9 |

### J.6 Budget, cost, time, packaging

**Call budget: recommend 250.**
- A frontier model needs an estimated 150–220 calls: about 25 exploring, 40–60 writing, 15 linting and fixing, 60–100
  testing hard behaviours on the dev world.
- 1.0 used 50–72 of 150 for a third of the scope.
- 150 would make the score a speed test, which frontier §5.3 warns against.
- More than 300 lets weak models flail expensively.
- `bench_budget.CALL_BUDGET` is global, so it needs a per-tier field (integration D5).

**Per-run model cost** (OpenRouter prices from the brief):

| entrant | estimate | basis |
|---|---|---|
| GPT-6.1 Sol | **$4–9** (point about $6); wall time about 50–90 min | 1.0 Sol cost $0.72 in 13 min. 2.0 is about 4–5× the calls at about 1.7× average context (≈ 15–30 M prompt tokens, about 95% cached at $0.10; about 1–2 M uncached at $2) plus 150–350 k output at $10 |
| Opus 5.5 | about $8–18 | 2× Sol on every token class (NOW.md) |
| cheap model (GPT-6 Luna, $0.10 / $0.50 / $0.01) | **$0.3–0.9** | Luna's SB7.2 profile at 250 calls |

Scoring costs no model calls (scripted LLM).

**Scoring time:** about 20–30 min per tree, serial (§F.6). Estimated; measured at freeze.

**Packaging added**, on top of 1.0's kit (app-modules about 115 MB, lint-modules 172 MB measured):

| item | size | status |
|---|---|---|
| `@forge/react` 12.3.0 closure | ≈ 144 MB unpacked (uikit#33) | registry-metadata sum; confirm with one `npm ci` + `du` |
| `@forge/sql` 4.0.7 | < 5 MB | — |
| local TiDB-compatible SQL engine | ≈ 100–250 MB | unmeasured; spike P21 decides |
| Confluence mock, UI Kit host, world generator, probe | < 5 MB | code |
| total added | **≈ 150 MB without SQL, ≈ 300–400 MB with it** | |

---------------------------------------------------------------------------------------------------------------------

## K. Open decisions for the owner

| # | decision | options | my recommendation and why |
|---|---|---|---|
| K1 | call budget | 150 (shared with Gauntlet) / 250 / 300 | **250**. The frontier needs about 150–220, and the score should measure engineering, not speed. This requires a per-tier budget field |
| K2 | Forge SQL | in (engine spike, +100–250 MB) / out (CES aggregates) | **in, if P21 passes**. It is a major GA capability and adds migration discipline. Fall back without regret, because the design is complete either way |
| K3 | Confluence static macro | in (probe-gated) / out | **in, if P20 passes**. It is the newest Preview module and forces the cross-user cache and per-product storage judgments. Weight is only 0.02, plus its leak vector |
| K4 | workflow validator (Preview) | in (probe-gated) / out | **in, if P11 passes**. It is the only synchronous, freshness-critical surface |
| K5 | app-managed permissions (Preview) | in (probe-gated) / out | **in, if P26 passes**. It is the realistic code-before-consent upgrade |
| K6 | public input size | ≤ 25 KB (1.0 rule) / ≈ 35 KB | **≈ 35 KB with an index**. 25 KB cannot state 19 modules' guarantees without dropping measured requirements, which would break "measured is stated" |
| K7 | brownfield v1 data | in / out | **in**. It is the vector the frontier study rated "a kind they have not been tested on", at moderate build cost (data only, no v1 code) |
| K8 | scoring time | ≤ 30 min per tree acceptable? | yes. If not, W1 and W2 drop to one derived world |
| K9 | critical floor | 0.6 everywhere (1.0) / 0.7 for the new criticals | keep 0.6 for leak, escalation, forgery, tenant and duplicate; `q_pool_exhausted` is already continuous |
| K10 | RoA | require full eligibility / require "no egress plus static trigger" | **the latter**, because SQL residency is unverified ([S] `dareCompliant`) |
| K11 | Forge 1.0's fate | fix and re-score / freeze with a note | must be settled before the site flip (integration §7 step 0). It does not block the 2.0 build |
| K12 | Phase 4 hardening trigger | Opus-class plans miss fewer than 2 of the 7 named mechanisms | adopt. Harden by scale, freshness and world kinds, never by trivia |
