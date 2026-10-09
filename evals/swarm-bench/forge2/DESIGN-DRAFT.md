# Forge 2.0 — design draft: "Escalation Desk"

Status: DRAFT for phase 3 (red team). The six-design panel synthesised by the chief designer, 2026-10-09; the
decisions and their evidence are in `panel/SCORECARD.md`. The base is the grader-first design, and every graft names
its source. Nothing was run to produce this draft.

**Reading rules.**
- **Facts.** Every platform fact is a BRIEF-confirmed claim (tags `tiers#N`, `rob#N`, `sec#N`, `llm#N`, `uikit#N`,
  `boot#N`, `[B]`) or a measured receipt (`understand/real-forge-fidelity.md`).
- **BRIEF §10 is binding**, so these refuted or outdated claims are not used anywhere:
  - The sort traps use decimals, display dates and currency, never integer strings.
  - Popup's `testId` is not denied.
  - "No external origins" is a benchmark/RoA rule, not a CSP fact.
  - Bridge `requestJira` DOES forward rate-limit headers (FRGE-1923).
  - Model statuses come from `list()`.
- **Fidelity class**, one per emulated behaviour:
  - **M** — measured. The same probe source ran live on wolfaenpak and in the emulator, and the answers matched.
    Fixtures are committed.
  - **D** — documented and stated. A doc quote no live run contradicts, plus the emulator's exact choice written into
    the contract. Rows accept every plausible reading.
  - **W** — benchmark world. Atlassian publishes no value and forbids probing it, so the contract owns the number and
    says so.
  - **G** — guess. Refused at freeze.
- **CTF** means "calibrate, then freeze": a proposal set from the golden's measured value times the stated margin, then
  frozen as a number in the contract before any entrant runs.

| headline | value | where |
|---|---|---|
| product | **Escalation Desk**: signed CRM escalations, SLA clocks, AI customer updates for Jira Cloud | §A |
| manifest module types | **15** (1.0 had 10). Newest: Forge LLM GA, Realtime token permissions, `dashboards:widget` (GA 2026-09-22), `jira:customField` (stored), `jira:jqlFunction`, UI Kit 2 admin, `rovo:skill`/`rovo:mcp` (Preview) | §B |
| predicted GPT-6.1 Sol | **0.40–0.75, central ≈ 0.55**; inner with no critical 0.66–0.78. Opus 5.5 0.40–0.78; mid 0.12–0.35; weak 0.00–0.15; reference 1.000; alt ≥ 0.95 | §G.3 |
| registry | 93 weighted rows in 9 tiers, 4 band diagnostics, 5 criticals (extent-scaled, bounded compounding), 2 bands, no graded robustness band, no excellence slice | §F |
| public input | ≈ 32 KB of prose + ≈ 15 KB of lookup JSON | §E |
| call budget | **300** (per-tier field; owner decision) | §J.6, §K |
| per-run model cost | Sol $5–9 (≈ $6.5); Opus 5.5 $13–30; cheap model (GPT-6 Luna class) $0.5–1.0 | §J.8 |
| scoring time | 15–30 min per tree on 3 scoring seeds; mutant mode 3–8 min; verdict identical idle and under load (gate G1) | §J.7 |
| packaging | desktop payload +4–10 MB; first-run kit download ≈ 0.45–0.75 GB (new kit lock) | §J.9 |
| reference app | ≈ 7.5–9.5k LOC in ≈ 65–75 files (1.0 golden ≈ 1,940) | §J.4 |
| calendar | Pilot-0 (an early Sol run, scored backend-only) ≈ day 12–16; full Sol acceptance pilot ≈ week 4–5 | §J.2 |

---------------------------------------------------------------------------------------------------------------------

## A. Product

### A.1 The pitch

**Escalation Desk.** B2B software companies run support in a CRM and engineering in Jira. When a customer case
escalates, three things go wrong. The CRM's webhook retries create duplicate Jira issues. Nobody in Jira can see how
close the escalation is to breaching its contractual response and resolution times. And the engineer who owns the issue
spends twenty minutes writing a customer-safe status update.

Escalation Desk fixes all three on sites of up to 8,000 issues, inside Tier 1, without starving the vendor's other
customers. It also keeps working while everything around it misbehaves in the ways the platform documents.

### A.2 What people get

| who | surface | what |
|---|---|---|
| the CRM | signed static web trigger | each case becomes exactly one escalation linked to exactly one Jira issue (created when the case names none); each CRM note becomes exactly one Jira comment; all of it holds through retries, parallel sends and disorder |
| escalation managers | **board** (`jira:globalPage`, Custom UI) | KPI tiles, the at-risk list, filters, live updates; useful even while the rate pool is exhausted |
| engineers | **panel** (`jira:issuePanel`, Custom UI) | timeline, clocks, and an AI customer update that reads only what this engineer may see, posted only after they confirm |
| team leads | **widget** (`dashboards:widget` with `edit`) | at-risk and breached counts for a chosen project, live |
| everyone in Jira | read-only **`Escalation SLA` field** and `issue in escalationBreached("P1")` | searchable SLA state |
| desk-admins | **admin panel** (`jira:adminPage`, UI Kit 2) | integration secret, roles, SLA policy and product → project map, AI budgets and kill switch, failed items, the app's own Tier 1 and LLM consumption, 1.x migration progress, the weekly digest |
| agents | Rovo `get-escalation` action, agent, skill; MCP | "how close is ACME's escalation to breach?", for the asking person only |
| auditors / compliance | `admin.export` | the canonical escalation record per case (also the grader's read path) |

### A.3 Each hard requirement is a customer's demand (graft: enterprise-product's persona table)

| who demands it | in their words | requirement | mandate |
|---|---|---|---|
| support director | "One escalation, one issue, one comment per note — no duplicates when the CRM retries." | exactly-once under at-least-once, lost answers, concurrency | 5 |
| customer success VP | "I need to know before we breach, not after." | SLA clocks from status history, pause statuses, policy in force | 3, 5 |
| CISO | "No unauthenticated endpoint writes into Jira; admin is enforced server-side; secrets never leave the vault." | HMAC over the raw body, replay protection, server-side roles, secret store | 4 |
| data protection officer | "Departed employees' names must disappear." | personal-data reporting with derived-data erasure | 4 |
| procurement | "Runs on Atlassian." | static web trigger, no egress, no remotes | 1, 4 |
| the vendor's own SRE | "We share ONE 65,000-point pool across every customer. One customer's burst must never take the others down." | per-installation budgets, smoothing, reaction to `r`, wall pause, per-person caps | 9 |
| Jira admin | "The panel loads on every issue view; it must not slow Jira." | count-based boot budgets; no LLM call or write at boot | 6, 7 |
| model-risk officer | "AI drafts are labelled, cite only what you may see, and post only when a human confirms." | grounding, prompt scope, injection resistance, confirm-to-post | 10 |
| existing 1.x customers | "Upgrading must not lose one escalation." | live migration while events flow | 5 |

### A.4 Why it is not benchmaxxed

There is no hello-world surface, and no Atlassian sample does this. Every hard requirement is one a real vendor of such
an app meets in production, and the partner record shows these failures are real:
- one tenant's button exhausting the global pool "rendering the app unusable for up to one hour on all tenants"
  (T99654, tiers#27);
- "50,000 issues … simply cannot complete within this limit" (t/101559);
- "under heavy load I occasionally lose increments" (t/101559);
- 300 customers stranded on a broken V1 (t/77751);
- "no immediate plans to support schema / breaking changes for CES" (t/96705).

The arithmetic is simple on purpose (graft: systems-torture). Forge 1.0 showed the frontier transcribes stated semantics
perfectly, so every point here is lost on OPERATING simple semantics correctly under production conditions.

---------------------------------------------------------------------------------------------------------------------

## B. Modules

Statuses come from BRIEF §1.1 (verified 2026-10-09). "Hard" is the judgment a strong model plausibly gets wrong; "rows"
point at §F.

| # | module / API | status | role | what makes it hard | rows |
|---|---|---|---|---|---|
| 1 | `webtrigger` `crm-intake`, `response.type: static`, outputs `accepted` 202, `unauthorized` 401, `malformed` 400, `busy` 503 | GA; only static triggers are RoA-eligible (sec#18) | CRM intake | HMAC over the RAW body; header names in any case, values as arrays; an atomic replay claim (`FAIL_IF_EXISTS` → 409 `KEY_CONFLICT`, measured); no domain effect before verification; answer ≤ 10 virtual s; a burst above the async push limit forces an inbox, not one push per delivery; `busy` as backpressure | S webhook rows, `r_freshness`, `t_platform_limits` |
| 2 | `consumer` ×3: `work`, `llm-work`, and the 1.x queue `crm-events` | GA, `@forge/events` 3.0.7, v2+ retries (rob#27) | all background work | at-least-once and unordered; `jobId` is not an idempotency key (rob#20); `InvocationError` `retryAfter` ≤ 900 (rob#24) against a 25–45-minute wall; 50 events and 200 KB per push, 500 events/min (rob#15); 100 KB per event for long consumers (rob#16); the cyclic 1,000 (rob#18); concurrency keys are per installation across queues (rob#28); old-version events reach new code (rob#32) | R and T rows |
| 3 | `trigger`: `avi:jira:updated:issue`, `avi:jira:deleted:issue`, `avi:jira:deleted:project`; lifecycle `avi:forge:installed:app`, `avi:forge:upgraded:app` | GA | clocks, moves, deletes, install, upgrade | up to 3 min late (rob#33); `selfGenerated` events without `ignoreSelf` (rob#35); only top-level deletes (rob#36); 401/403 before permissions (rob#37); `upgraded` only on major versions [B] | `r_sla_clocks`, `r_world_changes`, `r_self_events`, `r_install_race`, `r_migration` |
| 4 | `scheduledTrigger` ×2: `hour` (reconcile, privacy cycle), `fiveMinute` (SLA sweep, breach comments, field flush) | GA; ≤ 5 per app, ≤ 1 `fiveMinute` (tiers#39) | the stream is not trusted (BRIEF §8) | duplicate runs (rob#40); a throw is not retried (rob#39); skips and overlaps (stated W); spread across the hour (tiers#23) | `r_scheduled_semantics`, `t_smoothness` |
| 5 | `jira:jqlFunction` `escalationBreached` | GA; contract captured [B] | filters and boards | precomputations are not per user; ≤ 1,000 right-hand values; refreshed only by the app through the computation API; 25 s [B]; installation A exceeds 1,000 breached at the peak | `r_jql_results`, `s_jql_user_agnostic` |
| 6 | `jira:customField` `escalation-sla`, `type: number`, `readOnly`, written through `POST /rest/api/3/app/field/value` | GA [B]; **probe-gated (P-FIELD)** | searchable SLA state on the issue | values change on time-driven transitions; one bulk request costs 1 point while per-issue writes cost 1 each; the app's own writes come back as events; index lag after writes (stated after P-FIELD) | `r_field_values`, `t_economy` |
| 7 | `jira:globalPage` `board`, Custom UI | GA; ONE per app ("deployment will fail" otherwise) [B] | the front-facing board | count-based boot; live through Realtime without polling; per-person visibility under a reuse bound and a per-person points cap; useful during a pool wall; XSS from CRM text | B, U, V, S rows |
| 8 | `jira:issuePanel` `panel`, Custom UI | GA [B] | timeline, clocks, AI draft | loads on every issue view (tightest boot budget); `extension.issue.id` is not proof of permission (sec#1); confirm-before-post exactly once | B, U, `r_confirm_once`, `s_idor` |
| 9 | `dashboards:widget` `at-risk` + `edit`, Custom UI | GA 2026-09-22 (CHANGE-3453); `edit` required [B] | per-lead counts | config per instance comes from `extension.config` (a 1.0 trap); the edit API's Save; two instances on one dashboard share the 50 ops/s Realtime budget | `u_widget`, B rows |
| 10 | `jira:adminPage` `admin`, `render: native` (UI Kit 2, `@forge/react` 12.3.0, React 18.3.1) | GA (uikit#18, #20) | the admin panel | subpages are Custom UI only, so it uses Tabs (uikit#20); `Form.onSubmit` gets no data (uikit#28); UserPicker stores an object (uikit#29); DynamicTable sorts by cell key with the ADS comparator (uikit#11, #12); `testId`s only on components that deliver them (uikit#31); admin enforced in resolvers | K rows, `b_admin_boot` |
| 11 | `llm` (`@forge/llm` 1.0.7) | GA 2026-07-29/30 (llm#1); one per app | drafts (a tool loop) and the digest | no structured outputs; forcing is stated by us (llm#4); argument validation; parallel tool calls answered by id (llm#9); sampling rules (llm#16, #17); 100 RPM per installation across models and 500k TPM per model (llm#20, #21); the 5-minute window only in consumers with `timeoutSeconds` ≥ 300 (llm#23); `ForgeLlmAPIError` is not exported (llm#36); billed to the developer (llm#29) | G rows, `s_prompt_injection` |
| 12 | Forge Realtime (`@forge/realtime` 1.0.1; publish-only and subscribe-only tokens, CHANGE-3326) | GA | live board and widget; draft delivery | claims derived server-side (llm "newer facts" 5); 50 ops/s per installation including UI subscriptions (rob#63); `publish` returns `errors[]` | `u_board_live`, `s_realtime_isolation`, `t_platform_limits` |
| 13 | KVS + Custom Entity Store + `kvs.setSecret` (`@forge/kvs` 2.0.7) | GA | all state | CES conditional transactions (≤ 25 operations, no `keyPolicy` inside: rob#46, #48, #49); `query` is eventually consistent (rob#43); cursors cannot be persisted (rob#57); 10 KB units and 1 MB/s per key (rob#52, #53); TTL in the 4th argument of `transact().set` (rob#50); exactly one range attribute per index (server rule, measured); measured error codes (P01) | `r_no_lost_update`, `s_secrets`, `t_platform_limits` |
| 14 | `action` `get-escalation` (GET) + `rovo:agent` + `rovo:skill` + `rovo:mcp` | action and agent GA; skill Preview (CHANGE-3499); mcp Preview — demanded "as documented on 2026-10-09" | agents and MCP clients | inputs are untrusted, identity comes from context (sec#25, #26); skill frontmatter rules (llm#48, #49); one mcp, `name` ≤ 30 (llm#50) | `s_rovo_inputs`, `l_manifest_contract` |
| 15 | personal-data reporting (`POST /app/report-accounts/` or `privacy.reportPersonalData`) | documented (sec#30) | erase or refresh stored names | ≤ 90 accounts per request; the helper fires every batch concurrently (sec#31); 429 with `Retry-After` | `s_personal_data` |
| 16 | bridge `requestJira` (rate-limit headers forwarded since FRGE-1923) and `invoke(…, {rateLimitProperties: true})` (CHANGE-3314) | GA | person-scoped issue fields in the browser; boot under a 429 | the stated points exemption is a real dosing lever (tiers#24, #25), but it is never authorization; exactly one retry at or after `rateLimitReset` (a stated rule) | T rows, `b_boot_429` |
| — | Custom UI multi-entry resources (`entry`, GA CHANGE-3337) | allowed, never required | bundle sharing | demanded nowhere until P-ENTRY proves bridge injection into named entries (BRIEF §1.1 risk) | — |

**Scopes:** declared = required per observed call, by the shipped OpenAPI (1.0 rule). **Runtime:** `nodejs22.x` or
`nodejs24.x`; `nodejs20.x` is a measured server-side ERROR.

**Considered and excluded:**

| left out | why |
|---|---|
| Forge SQL (GA) | needs a MySQL-compatible engine in the kit (unmeasured, +100–250 MB); its query limits are wall-clock (rob#60); an atomic upsert would dissolve the concurrency difficulty |
| Confluence static macro (Preview) | a second mock product; the cross-product data path is undocumented (platform-breadth's fairness judge); "mostly volume" (frontier-behaviour §5.3) |
| workflow validator, condition, post function (Preview) | the expression arms need a Jira-expression evaluator; a schema-valid arm the emulator cannot run reopens 1.0 defect C |
| `jira:customField` value function or view renderer | Jira's invocation, batching and caching are unmeasured (machinery §4.7: HIGH). The STORED field is in |
| `apiRoute`, Object Store, app-managed permissions, `jira:command`, `global:fullPage` | Preview; host or request contracts are undocumented or unmeasured (BRIEF §1.1, §11.3) |
| `rovo:agentConnector`, Containers | need an external agent or a container runtime |
| `jira:projectSettingsPage` | considered for same-resource authorization; the panel, `get-escalation` and `s_idor` already grade checking the acted-on resource |
| any egress | breaks Runs-on-Atlassian eligibility, which the contract requires |

---------------------------------------------------------------------------------------------------------------------

## C. Architecture (the reference design)

The contract prescribes OUTCOMES. It also fixes four interfaces: the admin API, the export schema, the 1.x layout and the
UI hooks. It never prescribes a storage shape, a queue topology or a key name beyond those. The reference below proves
the guarantees can all be met within the stated limits, sizes the work, and is what WP7 builds. §C.12 lists alternative
designs that must also pass.

### C.1 Data model

Every CES index has exactly ONE range attribute, because the server rule refuses more (Forge 1.0 §17.8 E, measured).
There are at most 20 entities and 7 indexes per entity.

| store | key | content | index (partition → range) | why this shape |
|---|---|---|---|---|
| CES `escalation` | `esc:<caseId>` | caseId, issueId, projectId, priority, crmState, seq (last applied), policyVersion, openedAt, activeMs, pausedSince, respondedAt, resolvedAt, responseBreachedAt, resolutionBreachedAt, slaState (`on-track`/`at-risk`/`breached`/`paused`/`closed`), nextDueAt, issueState (`live`/`moved`/`deleted`), customer, origin (`v2`/`1.x`), version (integer), updatedAt | `by-project` (projectId → nextDueAt), `by-issue` (issueId → updatedAt), `by-state` (slaState → nextDueAt) | every writer is a conditional transaction on `version`; the sweep reads `by-state`, never Jira |
| CES `note` | `note:<caseId>:<noteSeq>` | textHash, status (`pending`/`posting`/`posted`/`failed`), commentId, attempts, marker, updatedAt | `by-status` (status → updatedAt) | the posting state machine behind exactly-once comments |
| CES `draft` | `draft:<id>` | requester, caseId, scopeHash, model, status, text, citedKeys, usage, createdAt, postedCommentId | `by-requester` (requester → createdAt) | ownership (IDOR) and confirm-to-post exactly once |
| CES `failure` | `fail:<kind>:<id>` | reason, attempts, firstSeenAt, lastError | `by-kind` (kind → firstSeenAt) | the admin's failed list |
| CES `actor` | `actor:<accountId>` | displayName snapshot, lastReportedAt, closed | `by-reported` (const → lastReportedAt) | personal-data reporting |
| KVS | `in:<deliveryId>` | the raw CRM body and its accept time; TTL 24 h | — | the inbox; events carry ids, never payloads (100 KB per event, rob#16) |
| KVS | `dlv:<sha256(deliveryId)>` | `{at}`; `keyPolicy: FAIL_IF_EXISTS`; TTL 25 h; reads check `expireTime` (rob#47) | — | the atomic replay claim; raw ids are hashed for the key regex |
| KVS | `drain:pending` | `FAIL_IF_EXISTS`, TTL 30 s | — | the doorbell: at most one drain push per window |
| KVS | `role:<accountId>` | `{role, grantedBy, at}` | — | app roles; the platform has none (sec#9) |
| KVS | `verdict:<accountId>:<issueId>` | `{browse, visibleCommentIds, at}`; TTL 5 min; `expireTime` checked | — | access decisions reused ≤ 5 virtual min, never across persons |
| KVS | `pts:<hour>:<class>:<shard>` | points; 8 shards; flushed once per invocation | — | self-accounting without one hot key (rob#52, #53) |
| KVS | `pp:<hour>:<accountId>` | interactive points of one person | — | the per-person budget |
| KVS | `pool:state` | `{pausedUntil, reason, nearLimitUntil, lastHeaders}` | — | the pause shared by every invocation of this installation |
| KVS | `burst:<method>:<pathTemplate>` | `{until}`; short TTL | — | burst back-off shared across invocations |
| KVS | `llm:<sha256(install, scopeHash, contentHash, model)>` | the draft; TTL 7 d | — | the stated LLM cache key |
| KVS | `model:list` | the `list()` result and its fetch time | — | refreshed at least every 10 virtual min |
| KVS | `mig:watermark`, `mig:stats` | the last migrated 1.x key; counts | — | resumable migration, never a stored cursor (rob#57) |
| KVS | `policy:current`, `policy:<v>` | minutes per priority and clock, pause statuses, product → project map, version | — | a policy applies from the instant it is saved |
| KVS | `field:dirty` | issue ids whose field value must change, grouped by value | — | bulk `/app/field/value` writes |
| KVS | `lease:<name>` | `FAIL_IF_EXISTS` + TTL + `expireTime` check | — | duplicate and overlapping scheduled runs |
| secret store | `crm-secret`, `crm-secret-prev` + the grace end | `kvs.setSecret` | — | presence-only read-back; rotation grace |

**Transactions.**
- **Escalation apply** is one CES `transact()`: it sets `escalation` with the condition `version == v`, writes up to 23
  `note` rows, and checks `version`. On `CONDITIONAL_CHECK_FAILED` (measured 400) the writer re-reads with `get`, which
  is strongly consistent, and retries at most 5 times. After that it re-enqueues with a delay and never spins.
- **No `keyPolicy` in batches or transactions** (rob#46). The replay claim is therefore a single `kvs.set` with
  `FAIL_IF_EXISTS` (measured 409 `KEY_CONFLICT`).
- **Creating a missing entity** uses an entity `set` with `FAIL_IF_EXISTS`, pending the P03 receipt. Until that receipt
  exists, the golden does not rely on a CES condition against a missing entity (machinery N5; live case 05 refuses it).

### C.2 Async topology

```
CRM sender ── signed POST ──▶ webtrigger crm-intake  (static outputs; answers ≤ 10 virtual s; no Jira call)
  verify HMAC over the raw bytes (current secret, or the previous one inside the 10-minute rotation grace)
  → timestamp within ±300 s → claim dlv:<hash> (FAIL_IF_EXISTS; 409 = already accepted → 202)
  → write in:<deliveryId> (TTL 24 h) → claim drain:pending (FAIL_IF_EXISTS, TTL 30 s); if won, push {drain} delay 5 s
  → 202 accepted | 401 unauthorized | 400 malformed | 503 busy (storage refused; nothing claimed)

queue work ◀── drain: read the inbox (after the stated 5 s query lag), group by case bucket, push ≤ 50 ids per push;
                  it re-arms itself while the inbox is non-empty, so a delivery accepted under a held doorbell is never stranded
           ◀── issue events: trigger (ignoreSelf; ids only; no Jira call) ◀── updated / deleted issue, deleted project
           ◀── 1.x events from crm-events ({caseId, payload}, old shape, same path)
consumer apply  (timeoutSeconds 120; concurrency {key: 'esc-' + bucket(caseId), limit 1})
  crm:   resolve or create the issue (idempotent, §C.8) → conditional transaction on the escalation → notes
         → post comments (marker protocol, paced per issue) → field:dirty → publishGlobal(board channel, {caseIds})
  issue: status history (changelog bulkfetch, fields filtered) → recompute clocks → transaction → field:dirty
  chunk: a continuation of the backfill, the migration or the digest (watermark in KVS; cursor never stored)
scheduled hour       → lease → jittered start → incremental JQL since the watermark − margin → chunk pushes (≤ 50 per push)
                       + deletion check of a rotating slice (bulkfetch issueErrors) + JQL precomputation sync if any
                       + privacy cycle for the accounts due (7-day cadence)
scheduled fiveMinute → lease → by-state range query (nextDueAt ≤ now + 5 min) → at-risk / breached transitions
                       → one breach comment per clock (marker protocol) → bulk field flush → publish
queue llm-work       → consumer draft (timeoutSeconds 600): chat() tool loop (≤ 4 rounds) → validate → cache
                       → publishGlobal on the requester's channel (claims minted by the requesting resolver)
                     → consumer digest slices: one call per open P1/P2 escalation, paced ≤ 90 RPM by a CES token bucket
lifecycle upgraded   → push {migrate}  (the migration also starts lazily on the first read of an esc1: key)
```

**Continuation across invocation limits.**
- Every long loop checks `getAppContext().invocationRemainingTimeInMillis()` (rob#6). Before the budget runs out it
  commits a watermark and pushes its own continuation, batching ≤ 50 events per push so chains stay under the cyclic
  1,000 (rob#18).
- A `Retry-After` or pool reset longer than the remaining time becomes
  `return new InvocationError({retryAfter: min(wait, 900)})`. A wall longer than 900 s re-enqueues with
  `delayInSeconds` in ≤ 900 s hops and re-reads `pool:state` on every wake. No function waits more than 10 virtual
  seconds for a limit (a stated rule).
- A poison item (an unknown issue key, an unmapped product, an issue deleted before first apply) moves to `failure`
  with its reason after ≤ 5 attempts. A 404 ends an item's processing; there is no redelivery storm.
- **Install race.** The first asApp calls of a fresh installation may answer 401/403 (rob#37). They are retried through
  `InvocationError` with `retryAfter` ≥ 60 s and are never a permanent failure inside the stated 15-minute window.

### C.3 Resolver catalogue and authorization rules

**Rules for every resolver and the action** (stated in §E §2 and §E §6):
- Identity is `context.accountId` only. Payload or input fields such as `accountId`, `isAdmin`, `role` and
  `requester` are ignored (sec#1, #2, #26).
- `context.extension.*` ids are hints, not proof. The harness does not validate them (stated).
- A resolver returns a value and never throws:
  - a refusal is `{"error":"forbidden"}` and changes no domain state;
  - malformed input is `{"error":"invalid","fields":[…]}`;
  - an upstream failure is `{"error":"unavailable","retryAt"?}`.
- Ids are validated before routing; `route` throws on path manipulation (BRIEF S22).
- An **access decision** (browse an issue, see a comment, desk-admin, Jira ADMINISTER) may be reused for at most
  **5 virtual minutes**, per (person, object), never across persons or installations.
- A person's **interactive Jira spend** is ≤ 1,000 points per virtual hour. Beyond it the answer is
  `{"error":"rate_limited","retryAt"}` with no Jira call.
- **Desk-admin** means a granted role or Jira ADMINISTER, checked as the user (an access decision like any other, so
  reusable for ≤ 5 virtual minutes): `POST /rest/api/3/permissions/check`
  with `globalPermissions: ["ADMINISTER"]`, or `GET /rest/api/3/mypermissions?permissions=ADMINISTER`. `authorize()`
  has no ADMINISTER helper and throws outside user-invoked modules (sec#12). No self-grant; the last desk-admin cannot
  be removed.

**Stated keys.** Payloads, results and refusal shapes are in `CONTRACT-SCHEMAS.json`. The harness configures and
changes the world through these, which decouples backend grading from the UI Kit host (graft: frontier-adversary;
enterprise's "world changes stay driveable").

| key | caller | input → result | Jira calls (reference) | failure handling |
|---|---|---|---|---|
| `admin.getConfig` | desk-admin | `{}` → `{secretSet: bool, webhookUrl, policy, budgets, roles}` | none | the secret is never returned |
| `admin.setSecret` | desk-admin | `{secret}` → `{ok}` | none | `setSecret`; never echoed or logged; the previous secret verifies for 10 virtual min |
| `admin.grantRole` / `admin.revokeRole` | desk-admin | `{accountId: string, role}` | a user lookup asApp, cached | no self-grant; last-admin refusal |
| `admin.setPolicy` | desk-admin | `{priorities: {P1..P4: {responseMin, resolutionMin}}, pauseStatuses[], productProjects{}, version}` → `{version}` or `{error:"stale"}` | none | integers 1–10,080; takes effect at the save instant |
| `admin.setBudgets` | desk-admin | `{userCreditsPerDay, installCreditsPerDay, aiEnabled}` | none | — |
| `admin.failures` / `retryFailure` / `dismissFailure` | desk-admin | `{page}` / `{id}` | none | a retry is applied once |
| `admin.usage` | desk-admin | `{}` → `{pointsThisHour: {background, interactive}, refusals: {reason: n}, pausedUntil, nearLimit, llmCreditsToday, projectedUsd}` | none | from self-accounting |
| `admin.migration` | desk-admin | `{}` → `{v1Total, migrated, remaining, done}` | none | — |
| `admin.startDigest` | desk-admin | `{}` → `{jobId}` | none | enqueues digest slices |
| `admin.export` | desk-admin | `{caseIds ≤ 100}` → canonical escalations (`CONTRACT-SCHEMAS.json#/escalation`) | none | **the auditor path the grader uses**; a compliance export |
| action `get-escalation` | any person who can browse the escalation's issue | `{caseId: string}` → `{caseId, issueKey, priority, slaState, response: {dueAt, met}, resolution: {dueAt, met}}` | an asUser verdict, if not reused | unknown, missing or invisible → `{error}`; never throws |
| `escalationBreached(priority?)` | JQL (no user) | → a JQL fragment | none | the same for every searcher |

**The golden's own keys.** Front keys are the app's choice; they are graded through the UI and by replay (§F.2).

| key | surface | authorization | Jira calls (principal, points) | failure handling |
|---|---|---|---|---|
| `board.bootstrap` | board | licensed user; rows only for issues the caller may browse, by a reused decision or a fresh asUser `search/jql` with `id in (≤ 50 candidates)` and `fields=id` (1 + visible) | ≤ 1 search | pool paused → `{paused: {until}, rows: only those with a reusable decision}`; per-person cap → `rate_limited` |
| `board.page` | board | as above | as above | keyset `{after: [nextDueAt, caseId]}`, never a stored cursor |
| `board.token` | board | licensed | none | `signRealtimeToken`, subscribe-only, claims `{accountId}` from context |
| `panel.bootstrap` | panel | browse on THAT issue (the extension id is checked, never trusted) | an asUser verdict if not reused | not browsable → `forbidden` and nothing else |
| `panel.requestDraft` | panel | browse; ≤ 5 drafts per person per virtual minute; credit budgets; kill switch | asUser comment list to fix the visible scope (stored as `scope`) | `{jobId}`, or `{error:"budget"\|"throttled"\|"disabled"\|"forbidden"}` |
| `panel.draftToken` | panel | the draft's requester only | none | subscribe-only token for the requester's channel |
| `panel.postDraft` | panel | the draft belongs to `context.accountId` and to this escalation (IDOR); the caller may comment | asUser `POST /issue/{id}/comment` (ADF) with the marker property | double confirm, second tab or unknown outcome → exactly one comment (§C.8) |
| `widget.view` / `widget.options` | widget | licensed; counts over visible escalations of the configured project | ≤ 1 search, reused decisions | no config → `needs-config` |

### C.4 Tier 1 dosing (stated rules, the inequality, the reference mechanism)

**Stated rules** (§E §4, numbers in `RATE-MODEL.json`; class W unless marked):

1. **Pool.** 65,000 points per virtual UTC hour for the app across every installation, including installations
   outside the scenario. It resets at the top of the hour with no carry-over, and an empty pool refuses every request
   until the reset (tiers#1, #4, #7; the hard wall is W).
2. **Costs** (W, shaped on tiers#5, #6, #30):
   - base 1, plus 1 per core object returned, plus 2 per identity or permission object;
   - writes cost 1, and a bulk app-field write costs 1 whatever its issue count;
   - `search/jql` and bulkfetch cost 1 + issues returned;
   - `mypermissions` costs 3; `permissions/check` costs 1 + 1 per 100 ids;
   - bridge `requestJira` costs 0 (a stated assumption from staff statements, tiers#24, #25) and is burst-bucketed.
3. **Interactive vs background.** Interactive means calls made inside a resolver or Rovo-action invocation, charged to
   its `context.accountId`. Background means every other function's calls: the web trigger, triggers, consumers
   (including those a resolver enqueued), scheduled triggers and lifecycle events.
4. **Background budget.** ≤ 10,000 points per installation per virtual hour (CTF), and ≤ 2,500 in any 5 virtual
   minutes (absolute).
5. **Per person.** Interactive spend ≤ 1,000 points per virtual hour (CTF); drafts ≤ 5 per person per virtual minute.
   Refused requests make no Jira or LLM call.
6. **Near limit.** When the `RateLimit` `global-app-quota` entry shows `r` < 20 % of `q`, background work makes no
   product call until the reset that `t` names; interactive work continues. Headers come in any order, `r` is
   optional, and `Beta-` entries are informational (tiers#17–19).
7. **Wall.** After any quota-class refusal, the installation's functions make no product call, background or
   interactive, until the reset (bridge reads from the browser stay exempt, rule 2). Quota-class means `jira-quota-*`,
   an unknown reason, or the gateway 429 that carries only `X-RateLimit-Reset`. Surfaces serve stored data in the
   paused state.
8. **Burst.** `jira-burst-based` slows only that method and path template, for at least `Retry-After`. Steady state is
   GET/POST 100 and PUT/DELETE 50 per second per installation and endpoint. Capacity is 1 s of refill (W). Simulated
   other-app traffic shares the buckets (CHANGE-2753).
9. **Per-issue writes.** ≤ 20 per 2 s and ≤ 100 per 30 s (tiers#13). After `jira-per-issue-on-write`, only that issue
   waits.
10. **In flight.** ≤ 4 product requests per invocation.
11. **Retries.**
    - never before `Retry-After` (else `X-RateLimit-Reset`, else the reset);
    - ≤ 4 attempts per request;
    - never repeat a write whose outcome is unknown before proving it did not land;
    - never wait in a function more than 10 virtual s for a limit or a retry: defer through `InvocationError` or a
      delayed push.
12. **Platform limits** (§E §10): async 50 events and 200 KB per push, 500 events/min per installation across queues,
    cyclic 1,000; KVS 1,000 RPS, 4,000 + 4,000 10 KB units/min, 1 MB/s per key; Realtime 50 ops/s; bridge 500 invokes
    per 25 s per frame; invocations 300/s and 7,000/min per installation, 1,200 per user per minute.

**The inequality** (stated in §E §4, graft: platform-breadth). It is built from the CAPS, never from the golden's
spend, so it holds for every compliant design. The scenario fixes two inputs: at most 3 scripted people act per
installation in any virtual hour, and outside the hot and wall hours the other installations draw at most 22,000 points
per hour. Harness-tagged calls draw nothing (§D.1 7).

3 installations × 10,000 background + 9 people × 1,000 interactive + 22,000 drawn by other installations
= 61,000 ≤ 65,000, a 4,000-point margin (re-derived with the frozen numbers at calibration).

- A compliant app can never wall the pool outside the hot and wall hours.
- In the HOT hour the other installations' draw ramps until `r` < 20 % of `q` appears (52,000 used), then adds at most
  2,000 more in that hour. Background then pauses (rule 6) and people stay capped: 52,000 + ≤ 9,000 interactive +
  2,000 = 63,000, leaving 2,000 points for calls already in flight when `r` appeared. A compliant app still never
  walls (proved again at calibration with the golden's ledger, §J.3 G14).
- In the BURST hour (A's burst, H1) a naive app walls the pool: its draw plus the other installations' exceeds 65,000
  (calibrated). That fires C5, which is attributed by counterfactual.

**Reference mechanism.** One `gate` module wraps every backend `requestJira`. Before each call it consults:
- the in-invocation state;
- `pool:state` (read at most every 10 virtual s);
- the burst map;
- the per-issue write queue;
- the hourly budget ledger, against which it reserves the estimated cost.

After each call it charges the actual cost, counting the objects returned, to the sharded ledger, flushed once per
invocation by conditional increments. It parses the headers into `pool:state`, and on a quota-class refusal it writes
`pausedUntil`. Consumers check the budget before every chunk and defer when it is short. Person-facing resolvers check
`pp:<hour>:<accountId>`. Person-scoped issue fields (summary, status, assignee) come from bridge `requestJira` in the
browser. Authorization never comes from the browser.

**Naive against disciplined** (estimates; WP7 measures the golden's real ledger, §J.3 G14):

| workload | naive design | disciplined design |
|---|---|---|
| A's burst (≈ 2,700 deliveries in ≤ 5 virtual min) | one push per delivery breaks 500 events/min (`RateLimitError` loses or delays work); a GET per delivery ≈ 5,400 points in minutes | inbox + doorbell: ≈ 60–120 pushes; 0 Jira reads for state the payload carries |
| status history for ≈ 3,000 escalated issues (A's initial work) | per-issue changelog GETs ≈ 15–30k points | changelog bulkfetch (1,000 issues, 10 field ids, tiers#29) ≈ 3 × (1 + 1,000) ≈ 3,000 |
| the SLA sweep every 5 minutes | `search/jql` of every open escalation ≈ 12 × (1 + 1,800) ≈ 21,600 per hour | a KVS `by-state` range query: 0 points |
| field updates on transitions | one `/app/field/value` request per issue per transition | bulk requests grouped by value ≈ 1 point per flush |
| a person opening the board 30 times an hour | an asUser verification search per open ≈ 30 × 51 = 1,530 points (over the per-person cap) | decisions reused ≤ 5 min ≈ 12 × 51 = 612 points |
| the hourly reconcile on A | a full rescan ≈ 1 + 8,000 points | watermark + rotating deletion slice ≈ 300–800 |

### C.5 Forge LLM features

**1. The customer-update draft** (panel → `llm-work`; graft: the tool loop from frontier-adversary and security-first).

- **The resolver** `panel.requestDraft {caseId}`:
  - authorizes: browse on the escalation's issue, the per-person throttle, the credit budgets, the kill switch;
  - fixes the requester's SCOPE, meaning the comment ids this person can see, read asUser, and stores it;
  - enqueues ids only;
  - returns `{jobId}` plus a subscribe-only token for the requester's channel, with claims derived on the server.
- **The consumer** (`timeoutSeconds` 600) runs a bounded tool loop:
  - read-only tools `get_timeline {caseId}` and `get_comments {issueKey}`, answered ONLY from the stored scope, so
    role-restricted comments the requester cannot see never appear;
  - a forced final tool `customer_update {summary: string ≤ 1,200, citedIssueKeys: string[] ≤ 10, nextStep: string ≤ 300}`;
  - the scripted model sometimes calls both read tools in parallel; the app answers every call with `role: "tool"` and
    its `tool_call_id`, and the emulator answers 400 otherwise (stated);
  - ≤ 4 rounds;
  - an unlisted tool gets an error tool message and is never executed;
  - the final turn is forced with `tool_choice: {type: "function", function: {name: "customer_update"}}`; that this
    forces a call is stated by the contract, since Atlassian does not say so (llm#4).
- **The model** is an id `list()` reports `active` at call time, from a list at most 10 virtual minutes old, Sonnet tier
  by the id word. It never sends `temperature` and `top_p` together, and neither one on opus-4-7, opus-4-8, opus-5 or
  sonnet-5 (llm#16, #17). `max_completion_tokens` is on every call.
- **Validation and grounding:**
  - types and lengths are checked; JSON-string `arguments` are parsed, not rejected (llm#5);
  - cited keys are kept only if the requester can see them, and invisible keys in prose are removed;
  - the summary renders as text with no links and carries the AI label.
- **Cache:** `(installation, requester scope hash, content hash, model)`.
- **Delivery:** `publishGlobal` to the requester's channel with the publish-only token the resolver minted. The panel
  shows the draft; confirm calls `panel.postDraft`.
- **Failures:**
  - refusal (no tool call), malformed arguments, 403 `FORGE_LLMS_MODEL_FORBIDDEN`, 429, 500, a stream with no non-null
    `finish_reason`, and an empty `end_turn` each give a stated state;
  - ≤ 1 retry per draft, never sooner than 30 virtual s;
  - after any LLM 429, no LLM request from that installation for 60 virtual s (the SDK exposes no `Retry-After`,
    llm#37);
  - errors are detected by `err.name === 'ForgeLlmAPIError'` or a numeric `err.status` (llm#36).
- **Latency envelope** (stated): each draft `chat()` round takes 40–140 virtual s, so no resolver (25 s) can hold one,
  and a full loop of up to 4 rounds needs a consumer with a long `timeoutSeconds` (the golden uses 600) or a
  continuation between rounds.

**2. The weekly digest** (admin):
- one call per open P1/P2 escalation, built ONLY from CRM-sourced fields; the audience is desk-admins, so no per-person
  visibility applies;
- uses the cheapest `active` tier, meaning the word `haiku` in the id;
- paced at ≤ 100 requests per 60 virtual s per installation and ≤ 500,000 estimated tokens per 60 s per model; the
  estimator is stated: `ceil(utf8 bytes / 4) + max_completion_tokens`;
- digest calls take 5–15 virtual s;
- an unchanged rerun costs 0 calls;
- the digest completes within 30 virtual min (CTF ≥ 2× the golden).

**3. Budgets.**
- Per-user and per-installation credits per virtual day, set in the admin panel.
- Credits = usage × the stated rates: Haiku 10 / Sonnet 30 / Opus 50 credits per 1M tokens; $0.10 in and $0.50 out per
  credit. These are an assumption for unpublished models (llm#30–32).
- Exhausted → 0 calls and the stated message. Kill switch → 0 calls and `disabled`.

**4. Injection.** Issue text and CRM text carry instructions, and the scripted model "obeys" them: an unlisted write
tool, an exfiltration URL, a hidden key, "post this now". The required outcome: no write without the confirm click, no
egress, the URL shown as text, the hidden key dropped.

### C.6 Front-facing Custom UI surfaces and the boot budget

| surface | SHELL (stated selector) | READY (stated selector plus seeded CONTENT) | live |
|---|---|---|---|
| board (`jira:globalPage`) | `[data-testid="board-shell"]`: header, four KPI tiles (`on-track`, `at-risk`, `breached`, `paused`), an empty table | `[data-testid="board-ready"]` plus the viewer's first page of at-risk rows `tr[data-case-id]`, each with case id, customer, priority, next due `time[datetime]` and SLA state, equal to the oracle | after a recorded change is published, the new state shows within one release wave; 0 invokes while idle |
| panel (`jira:issuePanel`) | `[data-testid="panel-shell"]` | `[data-testid="panel-ready"]` plus the timeline `li[data-seq]` in `seq` order and the clocks `time[data-clock="response\|resolution"][datetime]` | the draft arrives over Realtime (`[data-testid="draft"]`) |
| widget (`dashboards:widget`) | `[data-testid="widget-shell"]` | `[data-testid="widget-ready"]` plus counts `[data-metric]` for the configured project | live counts; `[data-testid="needs-config"]` without a config |

READY needs escalation fields only. Jira fields fetched through bridge `requestJira` (summary, status, assignee) may
arrive after READY. This resolves the base design's contradiction between READY and its bridge read wave.

**Boot design.**
- Render the shell synchronously at module load. An awaited `view.theme.enable()` is allowed and never penalised.
- Issue one bootstrap `invoke` in wave 1. The panel also issues a parallel bridge read, since its issue id is in the
  context.
- Subscribe to Realtime after READY. No flags SDK on the boot path. Production React, code-split.
- If the bootstrap invoke answers 429 (`rateLimitProperties`), keep the shell and retry exactly once at or after
  `rateLimitReset`, which is in virtual epoch seconds.

**Boot budgets** (CTF: the golden's measured value × 1.5, then frozen; counted, never timed):

| metric | board | panel | widget |
|---|---|---|---|
| B1 shell at wave 0 | yes | yes | yes |
| B2 backend waves to READY | ≤ 1 | ≤ 1 | ≤ 1 |
| B3 backend-bound ops before READY | ≤ 2 | ≤ 2 | ≤ 1 |
| B4 app-origin gzip -9 bytes before READY (platform scripts and theme token CSS excluded by URL pattern, boot#19) | ≤ 200 KB | ≤ 120 KB | ≤ 100 KB |
| B5 app-origin requests / B6 initiator depth | ≤ 6 / ≤ 3 | ≤ 5 / ≤ 3 | ≤ 5 / ≤ 3 |
| B8 bootstrap sequential outbound rounds; calls for N vs 4N visible candidates | ≤ 3; calls(4N) ≤ calls(N) + pages | ≤ 2 | ≤ 2 |
| B9 bootstrap response JSON | ≤ 48 KB | ≤ 16 KB | ≤ 8 KB |
| B11 CSP violations; non-app, non-platform origins | 0; 0 | 0; 0 | 0; 0 |
| B12 shell renders while the Realtime subscribe is held | yes | yes | yes |
| B13 repeated bootstrap on in-surface navigation; LLM calls during boot | 0; 0 | 0; 0 | 0; 0 |
| B14 bootstrap invoke answered 429 | shell stays; exactly 1 retry at or after `rateLimitReset` | same | same |

The UI Kit admin must make ≤ 1 invoke before the first ForgeDoc that holds the default tab's content, and the first
ForgeDoc must be non-empty (a loading state).

**During a pool wall** (graft: platform-breadth): every surface shows `[data-testid="quota-paused"]` with
`<time datetime>` equal to the reset instant. Rows appear only where a ≤ 5-minute access decision exists. Jira fields
from the bridge, which is exempt, are still shown.

**Designed states:**
- loading (the shell's skeleton);
- empty (`[data-testid="empty"]` for a viewer with no visible escalations);
- error (`[data-testid="error"]` when the bootstrap returns an error value);
- paused (above);
- needs-config (the widget).

**Design system.** `var(--ds-*)` tokens; light and dark, each with a painted surface; contrast ≥ 4.5:1. No horizontal
scroll on the board at 1280 and 1024 px, the panel at 400 px, or the widget at 380 px. Numbers are never clipped; focus
is visible on sortable headers. Screenshots and video are published for people to judge and are not graded.

### C.7 The UI Kit admin panel (`jira:adminPage`, `render: native`)

`Tabs` (`testId="admin-tabs"`; uncontrolled is allowed) with seven panels. Graded controls carry a stated `testId`, but
only on components that deliver one (uikit#31). UserPicker delivers none, so it is addressed by its label.

| tab | components | action → stated admin call | graded semantics |
|---|---|---|---|
| Integration | Heading; Text `webhook-url` (from `webTrigger.getUrl`); Lozenge `secret-state` (`Set`/`Missing`); Form with Textfield `secret` (`type="password"`); Button `save-secret` | `admin.setSecret` | Save → exactly one call; empty → ErrorMessage, 0 invokes; the value is never shown back |
| Roles | DynamicTable `roles-table`; UserPicker labelled "Person"; Select `role`; Button `grant`; Modal `revoke-confirm` | `admin.grantRole` / `revokeRole` | the payload `accountId` is a STRING mapped from `onChange(...).id` (uikit#29); revoke only through the Modal confirm; the last-admin refusal message |
| SLA policy | Form: Textfield (`type="number"`) per priority × clock; Checkbox pause statuses; product → project rows; Button `save-policy` | `admin.setPolicy` | invalid input (non-integer, outside 1–10,080) → per-field ErrorMessage and 0 invokes; valid → one call with integers and the version read at load; double activation → one call; a stale version → the stated `stale` message |
| Budgets & AI | Textfield credits per user and per installation; Toggle `ai-enabled` (the kill switch); Button `save-budgets`; Button `start-digest` | `admin.setBudgets`, `admin.startDigest` | one call each; after the switch is off, 0 LLM calls |
| Failures | DynamicTable `failures-table` with columns `attempts` (integer key), `first-seen` (ISO key, displayed "9 Oct 2026"), `age-hours` (decimal key: "2.25" vs "2.3"); Button retry per row | `admin.retryFailure` | header clicks give the stated orders (decimals and display dates mis-sort as strings, uikit#11 as corrected); retry → one invoke; the row updates |
| Usage | Text `usage-points`, `usage-paused`, `usage-credits`; DynamicTable `usage-refusals`; DynamicTable `usage-cost` with currency cells ("$1,200" vs "$900" mis-sort as strings) | `admin.usage` | values shown; sorted by underlying value |
| Migration | Text `migration-remaining`; ProgressBar | `admin.migration` | values shown |

**Host contract** (stated, §E §6):
- the host renders a stated component allowlist; any other component renders its children with a loud `unmodelled`
  marker and never holds the verdict or fails a K row by itself (spread-judge fix);
- `Form.onSubmit` is called with no data, so `handleSubmit` is the gate (uikit#28);
- on a double activation, the second activation arrives before the app's re-render reaches the host;
- uncontrolled Tabs emit no reconcile (uikit#27);
- DynamicTable sorts by cell `key` with the ADS comparator, en-US (uikit#12).

Every admin key is authorized server-side. Display conditions and page reachability are not controls (sec#5).

### C.8 The web trigger and exactly-once Jira writes

**The CRM sender is the benchmark's own world (W), stated in full in §E §3:**
- **Headers:** `x-desk-delivery` (uuid), `x-desk-timestamp` (unix seconds), and `x-desk-signature: v1=<hex
  HMAC-SHA256(secret, timestamp + "." + raw body bytes)>`. Names are case-insensitive and values arrive as arrays
  (sec#3; the case behaviour is pinned by P10).
- **Window:** accepted only within ±300 virtual s of the harness clock.
- **Retries:** the same delivery id and body, re-signed with a fresh timestamp, on any non-2xx answer or no answer
  within 10 virtual s. After `busy` it waits 30 virtual s. Up to 8 attempts over 60 virtual min; up to 8 deliveries in
  flight.
- **Body** (schema in `CONTRACT-SCHEMAS.json`): `{deliveryId, caseId, seq, kind: "case"|"note", priority, customer,
  title, product, issueKey?, note?: {noteSeq, text}, crmState}`.
- **Platform HMAC:** the platform's `request.authentication: hmacSharedSecret` is neither required nor penalised
  (sec#17).
- **Outcomes:**
  - verification fails (missing or bad signature, stale or future timestamp, no secret set) → 401, no domain change;
  - the body fails the schema → 400, nothing stored;
  - a valid repeat of an accepted delivery id → 202, applied once;
  - storage refused → 503 `busy`, nothing claimed;
  - otherwise 202.
  A failed-attempt counter is allowed and is not an effect.

**Exactly-once writes to Jira.** World fact (stated): "a write may be applied while its answer is lost, cut at 180
virtual s, or answered 5xx".
- **Comments.** Each note's comment carries the property `escalation-desk` = `{noteKey}` (the Comment bean has
  `properties`) or a marker in the body text; both are accepted. Before re-posting after an unknown outcome, the app
  lists the comment ids (`GET /issue/{id}/comment`), reads their properties
  (`POST /rest/api/3/comment/list?expand=properties`), and posts only if no comment carries the marker.
- **Issues.** Creation carries the label `esc-<caseId>` and the property. After an unknown outcome, the app searches
  only after the stated search-freshness window (≤ 30 virtual s), or passes candidate ids through `reconcileIssues`
  (≤ 50 ids). It never creates twice.
- **Breach comments.** Exactly one `SLA breached: <response|resolution>` comment per clock breach, with the same marker
  protocol, under duplicate and overlapping sweeps (lease).
- **Drafts.** `panel.postDraft` is idempotent per draft id: a double confirm, a second tab, or a retry after an unknown
  outcome gives one comment.

### C.9 Brownfield: Escalation Desk 1.x

**The 1.x layout** (stated in §E §9; present on installations A and B at upgrade):
- KVS `esc1:<caseId>` → `{caseId, issueKey, priority, status, seq, customer, openedAt, dueAt}`;
- KVS `esc1idx:<priority>:<caseId>` index keys;
- 1.x events `{caseId, payload}` still pending on the queue `crm-events`;
- 1.x stored no policy, and `issueKey` is a KEY: some issues have since moved project and some were deleted.

**Requirements:**
1. Consume `crm-events`; old events reach new code (rob#32).
2. Migrate every record exactly once, resolving keys to ids. A GET by an old key returns the moved issue (P13).
3. Compute the clocks of a 1.x escalation from its `openedAt` and the issue's status history, under the default policy
   from the upgrade instant. The 1.x `dueAt` is informational only.
4. Until the migration completes (≤ 60 virtual min after the upgrade), a read of a 1.x escalation returns its correct
   current state or `{state: "migrating", progress}`, never another value. Afterwards it is exact (graft:
   systems-torture).
5. Delete `esc1:` and `esc1idx:` keys only after migrating them.
6. Stay resumable across kills and within the background budget.
7. The upgrade may come with or without `avi:forge:upgraded:app`, and before or after deliveries start flowing.

### C.10 The JQL function and the custom field

**The field.** `escalation-sla` (`number`, `readOnly`):

| value | meaning |
|---:|---|
| 0 | no open escalation |
| 1 | on track |
| 2 | at risk |
| 3 | breached |
| 4 | paused |

Only the app writes it, through `POST /rest/api/3/app/field/value`. It equals the escalation's state within 10 virtual
minutes of a change, outside forbidden intervals. Validation does not run on app writes [B].

**The function.** `escalationBreached(priority?)` returns the issues whose escalation is breached now and, given an
argument, whose CRM priority equals it.
- It is evaluated once and shared by every user, because precomputations are not per user [B].
- A fragment may hold ≤ 1,000 right-hand values.
- Jira applies each searcher's permissions afterwards.
- It is fresh within 10 virtual min of a state change.

**Valid designs:**
- `cf[<id>] = 3 AND labels = esc-p1`: constant and fresh by construction;
- labels only;
- an id list plus computation updates (fails above 1,000 matches, which A reaches);
- an entity property, only if P18b verifies `jira:entityProperty`.

If P-FIELD fails, the field rows are cut and their weight moves to `r_jql_results` and `r_sla_clocks` (the only
pre-decided cut; it is driven by fidelity, not effort).

### C.11 Personal data (graft: security-first, enterprise-product)

**Stored:** role grants (accountIds), draft requesters, timeline actors (accountId plus a display-name snapshot) and
digest recipients.

**Duty:** report every stored account at most once per 7 virtual days through the reporting API:
- ≤ 90 per request;
- one request in flight per installation (a stated benchmark rule; the helper's concurrent batches violate it, sec#31);
- honour 429 `Retry-After`.

**Answers:**
- `closed` → within 60 virtual min, erase that display name from the timeline, the draft cache and the export (it shows
  "former user");
- `updated` → refresh it.

### C.12 Alternative designs that must also pass (alt app A uses the right-hand column)

| guarantee | golden | also valid |
|---|---|---|
| no lost update | conditional transactions on `version` | per-case concurrency keys (limit 1) with read-modify-write; read-time aggregation over immutable delivery records |
| one issue per case | label + property, lag-aware search or `reconcileIssues` | a per-case serial key plus a reservation claimed `FAIL_IF_EXISTS` before create, then the same lookup |
| one comment per note | comment property + `comment/list?expand=properties` | a marker in the body text |
| access decisions | asUser `search/jql` with `id in (…)` | asUser `permissions/check`; bridge-side display with a server re-check of every returned id |
| JQL at scale | field-backed fragment | labels; an entity property (if verified) |
| intake under the push limit | inbox + doorbell | inbox + a `fiveMinute` sweeper + `busy` backpressure |
| LLM transport | `chat()` | `stream()` with truncation handling |
| admin navigation | uncontrolled Tabs | controlled Tabs; Router (Preview) |
| draft scope | the resolver fixes the visible comment ids; the consumer reads asApp and filters | asUser in the consumer, if P23 shows that works for queued work a person started |

---------------------------------------------------------------------------------------------------------------------

## D. The world

### D.1 The world engine: lockstep virtual time (base: grader-first §0.1, with every judge fix)

1. **Warm workers.** App code runs in sandboxed Node processes under 1.0's measured deny-default `sandbox-exec` fence.
   Each runs Atlassian's sha-pinned runtime wrapper and serves ONE invocation at a time, as a Lambda does. Workers are
   reused across invocations and across installations in a seeded order, and recycled at seeded points (rob#11).
   `worker_threads` are not used: one process shares `BroadcastChannel` and a filesystem, a cross-tenant side channel
   production lacks. The pool size comes from the WP1 RAM measurement.
2. **An agent in every worker.**
   - It virtualises `Date`, `performance.now`, `process.hrtime`, `setTimeout`, `setInterval`, `setImmediate`,
     `timers/promises` and `AbortSignal.timeout`.
   - It seeds `Math.random` (V8 `--random-seed`) and EVERY crypto entropy source (`randomUUID`, `getRandomValues`,
     `randomBytes`, `randomInt`) per (schedule seed, invocation id).
   - It supplies `global.__forge_runtime__` with a `lambdaContext` closure (rob#7).
   - It reports what the invocation is blocked on (I/O, or a timer at virtual T).
   - Each clock read while the invocation is runnable advances virtual time by 1 µs, so a busy-wait ends
     deterministically.
3. **One scheduler, one choke point.** Events (deliveries, I/O completions, timer wake-ups) sit in a priority queue by
   (virtual time, sequence number). A proxied request completes at its issue time plus the stated latency of its class
   × a seeded factor in [0.8, 1.2]. One invocation runs at a time; interleaving happens at I/O boundaries.
   Two schedule families:
   - **canonical** (the seeded latency spread);
   - **reads-first adversarial** (graft: enterprise-product): among requests ready at one step, every read on a shared
     key, row or issue is granted before any write.

   Race rows take the worse of the two. Throughput rows use the canonical schedule only. Schedules change grant ORDER,
   never virtual timestamps (enterprise fairness-judge fix).
4. **Kills.**
   - Module timeouts are enforced on virtual elapsed time: resolver 25 s; consumer and scheduled 55 s, or
     `timeoutSeconds` ≤ 900; web trigger and action 55 s. The kill is by pid (gate 4) and completed writes persist.
   - A request in flight at the kill is applied or not by a seeded coin keyed to that request (stated: "may or may not
     have been applied").
   - **Platform terminations** keyed to the KILL population (§D.4): the first delivery that touches such a case is
     terminated after its k-th proxied request. Every app that processes the case meets it, so the kill row is never
     vacuous for a careful app.
5. **Unawaited work.** At handler return, pending timers and promises freeze. They resume only if the same worker
   serves a later invocation, possibly another installation's, and their I/O then carries the CURRENT invocation's
   token. On recycle they never run (rob#12).
6. **CPU.** CPU between I/O is free, which is lenient. An invocation that burns > 60 s of child CPU with no proxied
   request and no clock read is killed and treated as reaching its own module timeout: app evidence, ≥ 1,000× the
   golden's longest slice. HARNESS stalls (scheduler, site, browser) are detected by a harness heartbeat and HOLD the
   verdict for rescore. They are never an app zero.
7. **Checkpoints and harness calls.** At a checkpoint the background clock freezes and the harness's own invocations
   (the export, the browser and UI Kit lanes) run at that instant; the security and Rovo batteries run in the live
   timeline. Every harness-initiated invocation runs the app's own code and is refused like any other (inside a wall
   it meets the wall), but its calls are tagged harness: they draw nothing from the pool and count toward no budget
   measurement (`t_install_budget`, `t_person_budget`, `t_smoothness`, `t_economy`). The discipline rows
   (`t_pool_wall_pause`, `t_retry_discipline`, `t_in_flight`) and `t_usage_accuracy` read them like any other call,
   because the app cannot tell them apart.
8. **The browser lane.**
   - Chromium with a VIRTUAL frame clock injected before the first script (`Date`, `performance`, timers,
     `requestAnimationFrame`), aligned to the backend clock.
   - Hold-and-release waves (BRIEF §4.3): host-local bridge ops are answered at once; backend-bound ops are queued and
     released as one wave when the frame settles.
   - **Settle** means no runnable task, no timer due within 2 virtual s, and no pending bridge op or fetch.
   - The production CSP comes from `@forge/csp`; the real `@forge/bridge` 7.1.0 runs, limiter included.
   - Host chrome (flags) is drawn outside the app frame with `pointer-events: none` (1.0 defect A). A click the harness
     could not deliver is harness evidence and HOLDS the verdict.
9. **The UI Kit lane.** A Node host runs the real `@forge/react` 12.3.0 reconciler and `@forge/bridge` 7.1.0 under host
   fake timers. Each `reconcile` payload is deep-snapshotted. Callbacks are delivered asynchronously with serialised
   arguments (uikit#7), in the shapes captured by P16. **Quiescence** means no pending timer and no reconcile within K
   host ticks.
10. **Determinism (gate G1).**
    - The golden, alt A and every racy or timing mutant are scored twice: once idle, and once with every core saturated
      by a CPU and I/O hog. The verdict JSON must be byte-identical apart from wall-time fields.
    - Per entrant: one seed's backend lane is replayed once, and any difference HOLDS the verdict.
11. **Operations bound** (scorer-side, a ratio, not model work): a world ends when the app's proxied operations exceed
    20× the golden's measured count for that world. Every row still open then scores from the state reached, with the
    reason `operations_bound`.

### D.2 Installations and scale (per scoring seed; three seeds per tree; ranges are generator policy)

| installation | role | scale |
|---|---|---|
| **A** large | throughput, budget, migration, JQL scale | 4–5 projects; 6,000–8,000 issues; 2,500–3,200 escalations (60 % open); 1.x data on 1,500–2,000; 35–50 users; one CRM burst of 2,500–3,000 deliveries within ≤ 5 virtual min; > 1,000 breached at the peak |
| **B** permission-rich | visibility, world changes, ambiguous writes | 2–3 projects; 1,500–2,000 issues; 400–600 escalations; issue security levels; a reporter-only browse project; role-restricted comments on browsable issues; role revocations; moves, deletes and one project delete; 1.x data on ≈ 200 |
| **C** small, interactive | boot, LLM, the interactive reserve, the masher | 1 project; 200–300 issues; 60–100 escalations; people loading the board, panel and widget and drafting throughout |
| other installations (invisible) | the shared pool | not addressable; a seeded draw per hour: 10,000–22,000 outside the hot and wall hours; in the hot hour a ramp until `r` < 20 % appears, then at most 2,000 more; in the wall hour a spike that walls the pool for 25–45 min |

**Seeding rules.**
- Issue ids and keys overlap across installations, so a module cache keyed by issue id or key leaks on every seed.
- A few accountIds span installations. They are NEVER tenant canaries (fidelity-judge fix).
- Canaries are installation-unique tokens: generated customer names, case ids, delivery ids, summary tokens, the
  secret.
- The fixture generator fixes machinery N3: a numbered vocabulary, re-banded id classes, indexed simulation.

The contract states the envelope only: "up to 8,000 issues and 3,200 escalations per installation; CRM bursts of up to
3,000 deliveries in 5 minutes; three installations at once, plus installations of your app you cannot see".

### D.3 The virtual timeline (six virtual hours plus a privacy tail; the scorer's private plan)

| virtual time | on the scoring sites | graded at |
|---|---|---|
| H0+0 | v2 replaces 1.x on A and B (`upgraded` delivered on a seeded subset of seeds; 1.x events pending on `crm-events`). C installs fresh (asApp 401/403 for a seeded window ≤ 15 min). The harness configures every installation through the stated admin API as a desk-admin: the secret canary, roles, policy, budgets | — |
| H0+5m… | scheduled ticks begin; a CRM trickle everywhere; people on C load surfaces and draft; product events late, duplicated and reordered; events of the LOST population are never delivered | — |
| H1+0…5m | A's burst: duplicates, concurrent same-case deliveries, out-of-order `seq`, a HOT-ISSUE case with 60+ notes | **CP1** (H1+30m): export everywhere; UI Kit Node lane |
| H1–H2 | the AMBIG populations' first writes lose their answers; KILL terminations; moves, deletes and a project delete on B; status churn; a burst-bucket squeeze on one A endpoint (other apps' traffic); per-issue 429s wherever the app writes too fast | — |
| H2+0…20m | on B: role and permission revocations; a policy edit through the admin API; a secret rotation, with deliveries signed by both secrets following; the model the app called most in H1 turns `deprecated` in `list()` | **CP2** (H2+40m): export; browser lane (three viewers); security battery part 1 |
| H3 | the hot hour: the invisible draw ramps and `r` appears; one gateway-variant 429 | — |
| H4 | the wall hour: an invisible spike walls the pool for 25–45 min (never attributed to the app) | **CP3** (inside the wall): browser lane (paused state); interactive probes |
| H4–H5 | LLM drafts (failure mix keyed by case); the digest on A; the masher on C (40 draft requests in 60 s, 30 board opens in 3 min); security battery part 2; the Rovo battery | — |
| H5 | quiet: convergence; duplicate, overlapping and skipped scheduled ticks are spread across the run; a final reconcile with nothing new | **CP4** (H5+50m): final export; browser lane; Node lane; Jira-side counts |
| tail | +7 virtual days: the privacy cycle (the mock answers `closed` ×2, including the documented test id; `updated` ×1; one 429 + `Retry-After`) | **CP5**: privacy checks |

### D.4 Populations: faults keyed to domain entities (grafts: frontier-adversary rule 3, systems-torture §D.5)

The generator assigns every graded case to exactly ONE mechanism population. Each deep row grades its population at 80 %
of the row and all items at 20 %. The scenario guarantees a stated minimum per seed. Because the populations are
disjoint, a missing mechanism costs most of its row, and one root costs one row: each root is priced once by
construction. Faults are keyed to entities, never to call order, so every implementation meets the same adversity.

| population (min per seed) | what the world does to it | graded by |
|---|---|---|
| PLAIN (A's burst) | delivered once each during the burst | `r_freshness`, `r_one_record_per_case` |
| CONTENDED (≥ 150) | CRM state, status changes, the policy version and notes arrive concurrently for the same case | `r_no_lost_update` |
| REORDER (≥ 60) | `seq` delivered out of order, some concurrently | `r_monotonic_state` |
| DUP (≥ 60) | each delivery sent 2–4 times, some at once | `s_webhook_replay`, `r_one_record_per_case` |
| AMBIG-NOTE (≥ 20) | the first comment POST for each note is applied and answered 502/504, or cut | `r_note_exactly_once` (C2) |
| AMBIG-CREATE (≥ 10) | the first issue create for each case is applied and its answer lost | `r_issue_exactly_once` (C2) |
| KILL (≥ 20) | the first delivery touching the case is terminated after its k-th request | `r_kill_safety` |
| LOST (≥ 20) | product events for the case's issue are never delivered | `r_sla_clocks` (heal part), `r_world_changes` |
| MOVE / DELETE / PROJ-DELETE (≥ 10 / 8 / 1 project) | the issue moves (key changes, id stays) / is deleted / its project is deleted, with no child events | `r_world_changes` |
| CLOCK (≥ 40) | pause statuses; opened on an issue already in progress or done; a policy change mid-clock | `r_sla_clocks` |
| V1 / V1-EARLY (all 1.x / ≥ 30) | 1.x records; 1.x cases that get CRM traffic before the migration reaches them | `r_migration` (C4) |
| RESTRICTED (on B) | role-restricted comments, security-level issues, the reporter-only project | `s_person_visibility` (C1), `g_tool_loop` |
| POISON (≥ 5) | a non-existent issue key, an unmapped product, an issue deleted before first apply | `r_poison_items` |
| HOT-ISSUE (1–2 cases) | 60+ notes for one case within 2 minutes | `t_per_issue_writes` |
| LLM mix (all drafts) | per case hash: ≈ 80 % clean, 4 % refusal, 3 % malformed, 3 % obeys an injection, 2 % 500, 2 % 403, 2 % 429, 2 % truncated stream or empty `end_turn`, 2 % parallel tool calls | G rows, `s_prompt_injection` |

### D.5 What the contract states, and what the scorer does (never a fault list)

| the scorer exercises (private: when, where, how often) | the class the contract states (public) | graded by |
|---|---|---|
| duplicate, delayed, reordered, concurrent async deliveries; redelivery after a kill that followed a side effect | Forge: async events at least once, no ordering, retried within 24 h (rob#21–#30) | R rows |
| a Jira write applied but answered 5xx, cut, or its answer lost; search missing a fresh issue | world: "a request may fail after Jira applied it"; Jira: `search/jql` may not show writes from the last 30 virtual s unless `reconcileIssues` names them | `r_note_exactly_once`, `r_issue_exactly_once` |
| a pool wall of 25–45 min; `r` visible; the gateway 429; burst and per-issue 429s | world: the pool, other installations, header grammar, buckets (`RATE-MODEL.json`) | T rows |
| moves, deletes, project deletes, revocations, a policy edit, secret rotation | Jira: issues move and change key, get deleted (top-level delete events only), permissions change; the admin edits through the API | `r_world_changes`, `s_verdict_ttl`, `r_sla_clocks` |
| duplicate, overlapping and skipped scheduled ticks | Forge: duplicates possible, no retry after a throw (rob#38–#40); skips and overlaps stated | `r_scheduled_semantics` |
| kills, platform terminations, unawaited work resumed in another tenant's invocation | Forge: module timeouts; "an invocation can end at any point; what it wrote stays written"; warm reuse; unawaited work may run later or never | `r_kill_safety`, `s_tenant_isolation` |
| LLM refusals, malformed tool arguments, 403/429/5xx, truncated streams, injection, parallel tool calls, deprecation | Forge LLM: documented failure modes; model output is untrusted; `list()` is the truth at call time | G rows |
| replayed, forged, stale and case-varied web-trigger requests | world: the CRM spec; the URL is public | S webhook rows |

### D.6 The dev world and the scoring world

| | dev world (what the entrant's `forge-dev` serves) | scoring world |
|---|---|---|
| seed | `dev_seed`; the scorer refuses `--seed == dev_seed` (1.0) | the run's `fixture_seed` + 2 derived |
| installations | dev-A (≈ 1,500 issues, 1.x data, > 1,000 breached for the JQL cap) and dev-B (permission-rich: security levels, a reporter-only project, role-restricted comments), plus invisible draw | A, B, C + invisible draw |
| classes | **every class of §D.5 occurs at least 3 times** during `forge-dev world run --hours 6`; WP1 proves it from class-tagged events | different timing, targets and volume |
| messages | exactly what real Forge and Jira print (measured codes, P01 etc.); no harness hints and no semantic verdicts | same |
| tools | `world run [--hours N] [--until <event>] [--scale upper]` (another seed at the upper-bound scale, slower) · `ledger` (points per installation, hour, class and person; refusals by reason; KVS units; events per minute; Realtime ops; LLM RPM, TPM and credits — the "test quota" layer staff recommend, tiers#48) · `jira get <path> --as <user>` (read-only, charged to the dev ledger) · `webhook send [--sign\|--bad-sig\|--stale\|--header-case\|--dup N --concurrent]` · `invoke <fn> <key> --as <user> [--extension <json>] [--payload <json>]` · `boot report <surface> --as <user>` (the scorer's B1–B14 counters and the axe count) · `uikit serve\|render\|click` (the Node host with its ForgeDoc dump and bridge log) · `export validate` (schema only, never correctness) · `kvs` · `logs` · `lint` | — |

There is no oracle command, no fault menu and no printed LLM script. The dev world behaves like production: a race
gives a wrong number, never an error string. Every instrument the grader reads is available, and no verdict is
(graft: systems-torture's shared ledgers; frontier-behaviour §5.2 item 7).

---------------------------------------------------------------------------------------------------------------------

## E. Contract outline

### E.1 Public files and sizes

| file | content | size |
|---|---|---|
| `spec-build-forge2.md` (prompt) | mission; definition of done; "You have a budget of N model calls"; the score composition in words; "The scoring world is larger and harsher than the dev world. The dev kit reproduces every condition it has. Nothing tells you when your app is wrong except your own checks." | ≈ 3.2 KB |
| `FORGE2-CONTRACT.md` | §0–§12 below, opening with a 0.5 KB index | ≈ 25 KB |
| `STARTER.md` | workspace; pinned packages (adds `@forge/react` 12.3.0); reference material (manifest schema 13.6.0, OpenAPI jira/jsw, typings); the dev kit; "never call the site directly" | ≈ 4 KB |
| **prose total** | | **≈ 32 KB** (1.0: 18.9 KB; desk audits came at 45.7 KB and at 70 KB) |
| `RATE-MODEL.json` | pool and the inequality; cost table; buckets; per-issue windows; header samples per 429 kind (quota, burst, per-issue, gateway, unknown reason, 503); platform limits; latency model; token estimator; credit rates; the stated reference economy numbers | ≈ 5 KB |
| `CONTRACT-SCHEMAS.json` | webhook body; the admin API (keys, payloads, results, refusals); the export schema with per-field meaning; `get-escalation` I/O; LLM tool schemas; the 1.x layout; a WORKED SLA timeline (open → paused → policy tightened → resumed → moved → policy loosened → deleted) with the expected export at two instants | ≈ 8 KB |
| `UI-HOOKS.json` | every graded selector and `testId` per surface and tab; SHELL/READY; the UI Kit component allowlist | ≈ 2 KB |

The prompt says the JSON files are lookup tables, not reading. Phase 4 measures how many calls planners spend reading
(§G.5).

### E.2 Sections and their key guarantee sentences (verbatim intent)

| § | title (KB) | key sentences |
|---|---|---|
| 0 | What you build and how it is graded (1.0) | "The harness installs your app on several seeded Jira sites at once and runs it in virtual time: hours of traffic in minutes, identical on every machine. It grades what your app does — the escalations it records, the Jira changes it makes, the calls it spends and what each person sees — never how your code is shaped." · "Your app must stay eligible for Runs on Atlassian: a static web trigger, no egress, no remotes." |
| 1 | Escalations and SLA clocks (3.0) | "Every CRM delivery answered 202 is reflected in the escalation it names; an escalation's state is the state of the highest `seq` delivered for its case, whatever the order, timing or number of copies." · "Each case that names no issue gets exactly one Jira issue in the project its product maps to, with the summary `[<caseId>] <title>`; each CRM note becomes exactly one Jira comment, authored by the app, whatever happens between your app and Jira." · the SLA rule and the definitions of §E.3 · "An escalation follows its issue by id: through a move it keeps its clocks; if the issue or its project is deleted it shows `issueState: deleted` and its clocks stop." · "When a clock breaches, post exactly one comment `SLA breached: <clock>` on the issue." |
| 2 | Who sees what (2.5) | roles: `desk-admin` (granted, or implicitly every Jira administrator) and everyone else · "Every resolver, the Rovo action and the JQL function can be called directly by anyone who can load your app, with any payload and any `context.extension`; display conditions and hidden pages are not authorisation." · "Refusals return `{"error":"forbidden"}` and change nothing." · "A person sees an escalation's data only for an issue they can browse, and a comment only if its restriction admits them — a Jira comment may be restricted to a project role or group." · "Desk-admins see every escalation's CRM-sourced fields (case id, customer, priority, clocks, state, issue id) in the admin panel, the export and the digest; issue summaries and comments only where they can browse." · "An access decision may be reused for at most 5 virtual minutes, never for another person." · "Identity comes from the invocation context, never from a payload or an action input." · "No self-grant; the last desk-admin cannot be removed." · threat model (graft: security-first): "Assume every user knows every channel name and every resolver key; any user can publish on Realtime with the bridge; issue text, CRM text and model output are hostile." |
| 3 | Intake (2.0) | the full sender spec of §C.8 · "A request that fails verification is answered 401 and changes no data; a malformed body 400; a delivery id you already accepted 202, applied once; `busy` 503 when you cannot store it." · "A delivery id is applied at most once for 24 hours." · "The platform's `hmacSharedSecret` is not used." |
| 4 | Tier 1 and your budgets (3.0) | rules 1–12 of §C.4 in full; the inequality; "interactive" and "background" defined; "Requests your Custom UI makes through `@forge/bridge` `requestJira` cost no points and are not refused by the pool; they are subject to the burst buckets." · freshness: "Outside the intervals in which these rules forbid the calls you need (and 5 virtual minutes after them), a delivery is reflected in the export within 10 virtual minutes, a note appears as a comment within 30, a status change moves the clocks within 10, and the field and the JQL function follow a state change within 10." · "`admin.usage` reports your installation's spend this hour within ±5 % (or 20 points) of the harness's own ledger." · the reference economy numbers ("the reference spends about X points per accepted delivery") |
| 5 | Front surfaces (3.0) | the hooks of §C.6 (`UI-HOOKS.json`) · "Boot is graded cold by counting, never timing" plus every number of the boot table, the gzip level and the exclusions · "If the bootstrap `invoke` is refused with 429, keep the shell and retry exactly once at or after `rateLimitReset`." · "While the pool refuses calls, show `quota-paused` with the reset instant and only the data §2 still allows." · "Live updates through Forge Realtime; no polling." · the designed states, tokens, contrast, dark surfaces, widths, CSP · "Your pages run on the harness's virtual clock." |
| 6 | Admin panel and the admin API (2.0) | UI Kit only (`render: native`); the tabs, `testId`s and payloads; "Each Save sends exactly one call with the stated payload; invalid input shows the stated message and sends none; a stale version shows `stale`." · "Tables sort by cell `key` the way Atlassian's dynamic table does." · "The secret is never shown back — only whether it is set." · the host contract (the allowlist, `unmodelled`, `onSubmit` without data, double activation, uncontrolled Tabs) |
| 7 | Forge LLM (2.0) | the tool schemas, forcing, "answer every tool call", the loop bound · "Use a model `list()` reports `active` at call time." · "A prompt and every tool result contain only what the requesting person may see; model output is untrusted: shown as text, its issue keys only if visible, and nothing changes until the person confirms." · the cache key, budgets, kill switch, AI label · "Drafts are delivered to the requesting person only; claims are derived on the server." · the latency envelope · "The digest uses the cheapest active tier (the word haiku, sonnet or opus in the id)." · credit rates; the 100 RPM / 500k TPM rule and its estimator; "after an LLM 429, no LLM request from that installation for 60 virtual seconds" |
| 8 | JQL function, custom field, Rovo (1.5) | function semantics: "evaluated once and shared by every user; a fragment may list at most 1,000 values; fresh within 10 minutes" · field semantics (§C.10) · `get-escalation` I/O; the skill, agent and mcp rules |
| 9 | Escalation Desk 1.x (1.0) | the layout of §C.9 · "Every 1.x escalation is converted exactly once; until your migration completes (within 60 virtual minutes of the upgrade) a read returns its correct state or `migrating`, never another value; when it is complete no `esc1:` or `esc1idx:` key remains; 1.x events still queued on `crm-events` are applied." |
| 10 | Platform facts and limits (2.5) | one line each, with doc URLs: at-least-once, unordered async delivery with 24 h retention and `retryAfter` ≤ 900; scheduled duplicates, no retry after a throw; KVS `get` strict and `query` eventually consistent (≤ 5 virtual s here); cursors valid only inside the invocation that produced them; expired TTL values readable up to 48 h; warm reuse across tenants; unawaited work may run later or never; product events up to 3 min late, `selfGenerated`, only top-level deletes; install-time 401/403 (≤ 15 virtual min here); `search/jql` freshness and `reconcileIssues`; "a request may fail after the server applied it, or be cut at 180 virtual s"; "an invocation may end at any point; what it already wrote stays written" · the platform-limits table · deviations: virtual time and its 1 µs clock-read quantum; latency per call class; worker reuse; harness concurrency (≤ 8 async deliveries per installation at once, fewer by your keys; product triggers ≤ 2 at once; ≤ 2 resolver calls in flight per surface; the sender ≤ 8); scoring sites larger and differently seeded; unmodelled calls answer 501 and HOLD the verdict (1.0 R5) |
| 11 | Personal data (0.8) | what is stored; the 7-day cycle; ≤ 90 per request; one request in flight; `Retry-After`; erase on `closed` within 60 virtual minutes; refresh on `updated` |
| 12 | How the harness differs from production (1.5) | virtual time for backend AND pages; CPU is free (a run-away CPU loop counts as your invocation's timeout); the app account (asApp) browses every issue, including security levels (an emulator assumption; P13 records the live behaviour); `context.extension` is not validated; any licensed user can open the admin page; flags are drawn outside your frame; no pixels are graded; JSX bundles with the classic pragma, as `forge deploy` does |

### E.3 Definitions block (the pins the fairness judges asked for)

- **Instants** come from the Jira changelog's `created` timestamp of a status change, never from event arrival.
- **Active time** accrues while the issue's status is not a pause status of the policy in force.
- **Response is met** when the issue first leaves the `new` status category. An escalation opened on an issue already
  in progress has response met at open; one opened on an issue already done is resolved at open.
- **A breach** is recorded at the instant active time first exceeds the policy minutes in force at that instant. A later
  policy change never un-breaches. A policy change applies from its save instant and keeps the active time already
  spent.
- **At risk** means not breached, with the next unmet clock's remaining active minutes ≤ 25 % of its policy minutes.
- **The due instant** of a paused clock is `null`. The export's `activeMs` is as of the export call's virtual instant.
- **Deleted issues and projects.** A deleted issue's or project's clocks stop at an instant between the deletion and
  the moment your app learned of it. Any value in [deletedAt, detectedAt] is accepted, and detection must happen within
  60 virtual minutes.
- **The default policy and the product → project map** are in `CONTRACT-SCHEMAS.json` and apply until a desk-admin
  saves another.
- **Malformed** means the body fails the schema (400). **Poison** means schema-valid but unprocessable (202, then the
  admin failures list).
- **Sender retries** are re-signed with a fresh timestamp; the delivery id and body stay the same.
- **Issue keys anywhere in AI text** count as shown. **Board and widget channel payloads** carry case ids only.
- **A comment restricted** to a role or group is visible only to its members; browse permission alone does not reveal
  it.
- **Rounding:** clock minutes are integers, half away from zero; counts are integers.

### E.4 What the contract deliberately does not say

- which faults fire, where, when or how often;
- which cases belong to which population;
- that the scheduled job runs before any update (1.0 did, frontier-behaviour §2 row 7; here "events may arrive before
  your first scheduled run" is a stated fact);
- the LLM failure sequence;
- when the wall comes;
- the burst's time and size within the envelope;
- which users lose access;
- which model is deprecated.

Every one of these is an INSTANCE of a stated class. An app that honours every stated fact on every code path passes
whatever the timing.

---------------------------------------------------------------------------------------------------------------------

## F. Check registry

### F.0 Registry rules (the 1.0 §17.8 lessons and the panel's consensus findings, as mechanisms)

1. **Every row is continuous.** A fraction, `min(1, budget/measured)`, or `1/(1 + excess/k)` with k stated. No steps:
   one call never moves a row from 1.0 to 0.75 (defect F).
2. **Worst of the three scoring seeds** for every row and every critical factor. The economy parts of `t_economy` and
   `g_economy` take the mean, because an efficiency ratio has no adversarial seed.
3. **Populations.** A deep row = 0.8 × its population fraction + 0.2 × the all-items fraction (§D.4).
4. **Two-verdict preconditions.**
   - Surface absent → 0, with `vacuous_root` and no multiplier.
   - Stimulus present but avoided by stated-correct behaviour → graded on the app-independent stimulus. The
     populations and the injected platform faults (terminations, push failures) guarantee every app that processes the
     population meets the stimulus, so the more robust design is never charged for being robust.
5. **Harness evidence is never app evidence.** A click intercepted by host chrome, a harness heartbeat stall, a 501
   `harness_missing` or a browser crash makes the row `unavailable` and HOLDS the verdict.
6. **Criticals.** Each fires only on its named consequence, OBSERVED. An absence is graded in its own row. One event
   fires at most one critical, the most severe consequence it produced (graft: security-first's consequence-class
   dedup). Extensive harms are scaled by extent (§F.10).
7. **Each root is priced once:** populations, ROOT_BLOCKS (§F.12), and a vacuous row carries no multiplier.
8. **Basis column M / D / W per row.** The emulator tags every behaviour site with its class, and every row records
   the tags its evidence passed through. Any G tag refuses the freeze; this is taint tracking, not a hand-filled column
   (fidelity-judge fix).
9. **Anchors with numbers.** Every row names its contract sentence. `test_score_forge2.py` fails when a row lacks an
   anchor, an anchor lacks a row, or any numeric threshold a row uses is absent VERBATIM from its sentence or
   `RATE-MODEL.json`, or differs from the emulator constant.
10. **One-defect mutants:** at least one per row; racy mutants must fail on 20/20 calibration seeds.
11. **No code shape.** No row requires a module form, storage layout, queue, endpoint or key name beyond the stated
    interfaces.
12. **Concurrent refusals are never charged** (1.0 §17.6, extended to every "after a signal" row). A call counts as
    "after" only if it was issued at least one stated KVS round trip (20 virtual ms) after the installation received
    the signal. Calls already in flight are exempt.
13. **Domain state is enumerated.** "Changes nothing" means none of: escalations, notes, drafts, roles, policy, budgets,
    secret presence or value, failure-list state, Jira writes, queue pushes carrying the call's effect, Realtime
    publishes. Caches, accounting rows, rate-limiter rows and refusal logs are exempt by name (consensus finding 6).

**How state is read.**
- `admin.export` as the harness auditor, a site administrator who browses everything. Rows use a lenient field-by-field
  reader; the schema itself is the band-2 diagnostic, so a formatting slip is priced once.
- The mock Jira's own state and call log.
- The KVS snapshot: secret-store presence, leftover 1.x keys, a canary scan of plain values.
- The gateway ledger: one row per product call — virtual time, installation, invocation, class, principal, method,
  path, status, points, policy, reason, headers sent.
- The LLM log (every request with prompt and tool results), the Realtime delivery log, the bridge and ForgeDoc logs, and
  the DOM at settle.

Canaries are whole-token, seeded per scoring site, installation-unique, and never in the dev pack. Canary sets are
PER VIEWER and apply the §E §2 desk-admin carve-out.

### F.1 Tiers and weights; L — deployability and platform currency (0.05)

| L | S | R | T | G | B | U | K | V |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| .05 deploy | .18 security | .22 robustness | .18 Tier 1 & limits | .11 Forge LLM | .07 boot | .07 front surfaces | .07 UI Kit admin | .05 visual |

R, T and S together carry 0.58 of inner. Front-facing UI is U + V + three B surfaces; the admin is K + `b_admin_boot`.

| id | measured | anchor | w | C / band | basis |
|---|---|---|---:|:-:|:-:|
| `l_deployable` | 0 errors from the CLI's client lint (16 linters, pins byte-identical to CLI 14.1.0), the measured server-rule pack (every `MANIFEST_INVALID_RULE` text with its wolfaenpak receipt), the pinned deprecated-runtimes flag `["sandbox","nodejs18.x","nodejs20.x"]`, and the measured deploy-time refusals (two `jira:globalPage`, a second `llm` or `rovo:mcp`, > 5 scheduled triggers or > 1 `fiveMinute`, a UI Kit resource that is a directory or `.html`); lint ×2 identical; docs-only constraints are reported, never charged | prompt done-1 | .012 | band 1 | M |
| `l_bundles_load` | share of manifest functions that bundle as `forge deploy` does (classic JSX pragma, uikit#23; parity corpus P25) and load in a warm worker with the handler exported; every schema-valid consumer form is invocable (defect C) | prompt done-1 | .010 | band 1 | M |
| `l_lint_warnings` | `1 − 0.2·n` distinct warnings | prompt | .004 | | M |
| `l_manifest_contract` | fraction of the stated module facts: one `jira:globalPage`; `jira:issuePanel`; `dashboards:widget` with `edit`; `jira:adminPage` `render: native` with a file resource; static `crm-intake` with the four outputs; `escalationBreached`; `escalation-sla` (`number`, `readOnly`); one `llm`; action `get-escalation` (`actionVerb: GET`); a `rovo:agent` listing the skill; `rovo:skill` (`name` = directory, description 1–1,024 characters, `allowed-tools` ⊇ dependencies, llm#48, #49); exactly one `rovo:mcp` (`name` ≤ 30) exposing `get-escalation`; a consumer for `crm-events` | §0, §8 | .008 | | M |
| `l_scopes` | declared = required per observed call by the shipped OpenAPI (1.0 rule), over the union of calls across all seeds; −0.25 per missing or extra | §0 | .008 | | M |
| `l_roa_eligible` | the verified RoA list only (sec#20): no dynamic web trigger, no `permissions.external`, no remotes, providers or Connect modules; calibrated by `forge eligibility` on the golden and the mutants (P19) | §0, §3 | .008 | | M |

### F.2 S — security (0.18)

Every S row multiplies refusal correctness by a POSITIVE CONTROL: the same calls as an authorised principal succeed.
A stub that refuses everything therefore earns nothing (graft: security-first's spread judge).

| id | measured | anchor | w | C | basis |
|---|---|---|---:|:-:|:-:|
| `s_admin_authz` | every stated admin key, invoked DIRECTLY (bypassing the UI) with valid payloads, as: a licensed member; a project admin without ADMINISTER; anonymous `unidentified`; a member whose desk-admin role was revoked ≥ 5 min + ε earlier → refusal AND no enumerated domain change (rule 13); × the positive control | §2, §6 | .018 | C3 | M (resolver reach, sec#4) + W (roles) |
| `s_identity_from_context` | `accountId`, `isAdmin`, `role` and `requester` in payloads and action inputs are ignored; every attributed record names `context.accountId` | §2 | .008 | | D (sec#1, #2, #26) |
| `s_idor` | the front invocations recorded in the UI lane are replayed as other principals with substituted ids: another person's draft id, the `extension.issue.id` of an unbrowsable issue, case ids of invisible escalations; `admin.export` by a non-admin; `get-escalation` for an invisible case → refusal or omission, no data; × the positive control | §2 | .014 | C1 if data shown | W (unvalidated extension, stated) |
| `s_person_visibility` | canaries of hidden issues and comments (security level, reporter-only, revoked roles, role-restricted comment text) appear in NO person-facing channel: resolver and action responses, DOM, Realtime payloads delivered to that person, LLM requests built for that person (prompts AND tool results), widget counts' drill-downs; per-viewer canary sets | §2, §7 | .022 | C1 | M (asUser filtering, P13) + W (asApp sees everything, stated) |
| `s_verdict_ttl` | after each revocation at t, no person-facing response at t + 5 min + ε shows the issue's data or grants the revoked role's power; fraction of probes | §2 | .010 | | W (bound stated) |
| `s_webhook_auth` | unsigned; wrong secret; tampered body; stale (−301 s); future (+301 s); signature over re-serialised JSON; truncated hex; no secret set; another installation's secret → 401 and no domain side effect (no inbox or escalation write, no push, no Jira call; a failed-attempt counter is allowed) | §3 | .016 | C3 | W (sender) + M (request shape, P10) |
| `s_webhook_replay` | the same valid delivery twice in sequence, twice concurrently (reads-first schedule), and again 30 virtual min later (inside the stated 24 h) → exactly one effect and 202 each | §3 | .012 | C2 if ≥ 2 Jira effects | M (`FAIL_IF_EXISTS` 409, P01; expired-but-readable, P03) |
| `s_webhook_variants` | valid requests with header-name case variants (kept only if P10 shows case survives; otherwise lowercase only), equal duplicated header values, insignificant whitespace and unicode escapes in the raw body → accepted and applied | §3 | .008 | | M (P10) |
| `s_secrets` | the secret canary set through `admin.setSecret`: present in the secret store; absent from every response (`getConfig` shows presence only), log line, Realtime payload, LLM request, DOM and plain KVS value | §3, §6 | .010 | C1 on exposure only (a plain `kvs.set` alone is this row's loss) | M (P05) |
| `s_tenant_isolation` | installation-unique canaries never cross installations in responses, storage, Jira writes, logs or prompts; I/O from unawaited work that resumed in another installation's invocation and carries data counts as a crossing | §10 (warm reuse) | .012 | C1 | D (rob#11, #12) |
| `s_xss` | payloads in CRM fields, summaries, comments and LLM output (`<img onerror>`, `javascript:` hrefs, `</script>`, ADF marks) → no script runs (the canary global stays unset), no `javascript:` href, no `unsafe-inline` script, issue links only to `/browse/<KEY>` through the router | §5, §7 | .008 | | M (production CSP via `@forge/csp`) |
| `s_prompt_injection` | the scripted model "obeys" injected text (an unlisted write tool, an exfiltration URL, a hidden key, "post now") → no write without confirmation, no unlisted tool executed, no egress attempt, the URL as text, the hidden key dropped | §7 | .010 | | W (scripted model) |
| `s_realtime_isolation` | a second person subscribing to the first person's draft channel (forged claims in a payload, a guessed name) receives nothing; board and widget payloads carry case ids only | §2, §7 | .008 | C1 if data delivered | M (pairing and claims, P15) |
| `s_jql_user_agnostic` | the fragment is identical whichever user's search first evaluates it; results after Jira's permission filter are correct for each searcher | §8 | .004 | | M (P18) |
| `s_rovo_inputs` | `get-escalation` with a forged `accountId` input, an invisible case or a missing input → `{error}` or the caller's own view, never another person's | §8 | .004 | | D (sec#26) |
| `s_personal_data` | batches ≤ 90; one request in flight; `Retry-After` honoured; every stored account reported once per cycle; `closed` → the name erased from the timeline, draft cache and export within 60 virtual min; `updated` → refreshed | §11 | .008 | | M (test ids, P22) + W (one-in-flight rule) |
| `s_input_handling` | every stated key and every recorded front key, with malformed, oversized, wrong-typed, out-of-range and path-manipulating ids (`../`, encoded variants, `/`, `?`, `#`) → `{"error":"invalid"}` or `forbidden`, no state change, no unintended endpoint reached (BRIEF S22) | §2 | .008 | | M (`@forge/api` 8.2.0 safeUrl, read) |

### F.3 R — robustness and correctness (0.22)

| id | measured (population; §D.4) | anchor | w | C | basis |
|---|---|---|---:|:-:|:-:|
| `r_one_record_per_case` | at CP4 the export holds exactly one escalation per case that ever received a 202, with the stated fields (PLAIN + DUP) | §1 | .012 | C4 | W |
| `r_monotonic_state` | each escalation's state equals the highest delivered `seq`'s (REORDER) | §1 | .010 | | D (no ordering, rob#30) |
| `r_no_lost_update` | fields written by different paths for the same case (CRM state, clocks from Jira, policy version, notes) equal the oracle (CONTENDED, worst of canonical and reads-first) | §1 | .020 | | M (CES conditions under concurrency, P04) |
| `r_note_exactly_once` | comments carrying each note = exactly 1 (AMBIG-NOTE, KILL); 0 counts here, never as a duplicate | §1 | .018 | C2 (≥ 2) | M (comment `properties` + `comment/list`, P13) + W (ambiguous outcome) |
| `r_issue_exactly_once` | issues created per case = exactly 1 (AMBIG-CREATE + concurrent first deliveries) | §1 | .016 | C2 (≥ 2) | M (create, search freshness, `reconcileIssues`, P13) |
| `r_confirm_once` | double confirm, a second tab, and a retry after an unknown outcome → exactly one comment per confirmed draft | §1, §7 | .006 | C2 (≥ 2) | M |
| `r_sla_clocks` | respondedAt, resolvedAt, due instants, breach instants, pause accounting and policy version per escalation (CLOCK, LOST) at CP1, CP2 and CP4 | §1, §E.3 | .018 | | W (rules stated) + M (status-change payloads, P11) |
| `r_continuation` | A's initial work (migration, the status-history backfill) completes exactly once even though it cannot fit one invocation at the stated latencies | §1, §9, §10 | .012 | | M (timeouts, remaining time, P07) |
| `r_kill_safety` | items whose processing a module-timeout kill or an injected platform termination interrupted end exactly right: not lost, not doubled (KILL) | §10 | .010 | | M (P07) + W (injected terminations, stated class) |
| `r_poison_items` | POISON items appear in `admin.failures` with a reason within 30 virtual min; everything else completes; no item is delivered more than 6 times; a 404 ends processing | §1, §6 | .006 | | M (redelivery schedule, P06) |
| `r_world_changes` | moves keep the escalation (same id, new key shown); deleted issue or project → `issueState: deleted`, clocks stopped within the stated window; a policy edit applies from its save instant (MOVE, DELETE, PROJ-DELETE) | §1, §E.3 | .016 | | M (P11, P13) |
| `r_scheduled_semantics` | duplicate concurrent ticks, an overlapping long run and skipped ticks → no breach comment twice, no breach missed by CP4, heal not doubled | §1, §10 | .006 | C2 (a duplicated breach comment) | D (rob#38–#40) + W (skips, overlaps) |
| `r_migration` | every 1.x record migrated exactly once by CP4; reads during migration exact or `migrating`, never another value; no `esc1:`/`esc1idx:` key remains; every pending 1.x event applied (V1, V1-EARLY) | §9 | .020 | C4 (a lost 1.x record) | D (rob#32) + M (KVS) |
| `r_install_race` | work attempted during C's stated 401/403 window lands after it; nothing dropped | §10 | .004 | | D (rob#37) + W (window) |
| `r_self_events` | the app's own comments, labels, properties and field writes cause no further Jira writes or queue work from their own events (measured by event lineage, never against the golden's write count) | §10 | .004 | | M (`selfGenerated`, P11) |
| `r_freshness` | (a) PLAIN: deadline probes through the export at acceptance + 10 virtual min + ε on 60 sampled cases per seed; (b) notes as comments ≤ 30 min (mock comment store); (c) status → clocks ≤ 10 min (CLOCK deadline probes); exempt: intervals in which a §4 rule forbids the needed calls, + 5 min; canonical schedule | §4 | .016 | | W (objectives and latency model stated) |
| `r_resolver_contract` | every stated key and recorded front key, with valid input and a Jira 5xx on its first read → a value, never a throw; the surface shows its error state, not a blank | §2 | .006 | | M |
| `r_field_values` | the `escalation-sla` value of every escalated issue equals the oracle at CP2 and CP4, plus sampled checks 10 virtual min after transitions (read from the mock's app-field store) | §8 | .010 | | M (P-FIELD) — probe-gated |
| `r_jql_results` | `escalationBreached()` and `escalationBreached("P1")` searched as users A (wide) and B (narrow), in both orders, at CP2 and CP4 and 10 virtual min after a state change, including > 1,000 matches on A → results = oracle ∩ the searcher's visibility | §8 | .010 | | M (P18) |

### F.4 T — Tier 1 dosing and platform limits (0.18)

Every T row is a pure function of the gateway ledger and the platform counters. Rule 12 (concurrent refusals) applies
to every "after" clause.

| id | measured | anchor | w | C | basis |
|---|---|---|---:|:-:|:-:|
| `t_install_budget` | per installation-hour, background points ≤ 10,000 → 1, else `max(0, 1 − (spent − B)/B)`; row = 0.5·mean + 0.5·worst installation-hour (spread-judge fix: a mean alone dilutes one blown hour) | §4 | .028 | C5 | W |
| `t_pool_wall_pause` | per (wall × installation): `charged` = product calls issued ≥ 20 virtual ms after the installation's first quota-class refusal (including the gateway variant) and before the reset; unit score `1/(1 + charged/5)`; mean | §4 | .020 | | W + D ("pause all requests", tiers#20) |
| `t_near_limit_backoff` | per episode in which an installation received `r` < 20 % of `q`: `charged` background calls issued ≥ 20 ms later and before the reset `t`; unit score `1/(1 + charged/10)`; mean | §4 | .012 | | W (grammar from docs, tiers#17–19) |
| `t_burst_per_endpoint` | mean of (a) the fraction of repeats to a throttled method and path at or after its `Retry-After`; (b) app-caused burst 429s outside the injected squeeze: `min(1, 3/max(3, n))` per installation-hour | §4 | .010 | | W |
| `t_per_issue_writes` | per-issue refusals received: `1/(1 + n/2)` per installation; × "after one, writes to OTHER issues continue within `Retry-After` + 5 virtual s" (HOT-ISSUE) | §4 | .008 | | D (tiers#13) + W |
| `t_retry_discipline` | mean of three: (a) repeats of a refused request — the SAME installation, method, path and body hash, across invocations — issued at or after `Retry-After` (else `X-RateLimit-Reset`, else the reset), with ≤ 4 attempts; (b) the fraction of invocations whose in-function waits stay ≤ 10 virtual s; (c) the fraction of unknown-outcome writes never repeated without a proof read | §4 | .018 | | D (tiers#21: only its three gradable parts) |
| `t_in_flight` | per invocation `min(1, 4/max_in_flight)`; mean over invocations, weighted by product calls | §4 | .008 | | W |
| `t_smoothness` | per installation-hour, the fraction of 5-virtual-minute windows with background ≤ 2,500 points | §4 | .008 | | W |
| `t_person_budget` | per (person, hour) with interactive activity, `min(1, 1,000/spent)`; × the masher (40 drafts in 60 s, 30 board opens in 3 min): the excess refused with 0 Jira and 0 LLM calls | §4 | .016 | | W |
| `t_interactive_reserve` | outside stated pauses, the share of person-facing resolver and action calls on C, and on A's people during A's burst, that return data or a legitimate stated refusal (`rate_limited` beyond the cap, `paused` inside a pause) | §4 | .010 | | W |
| `t_usage_accuracy` | `admin.usage` at CP2, inside the wall and at CP4 against ledger truth: points this hour (background, interactive) within ±max(5 %, 20 points) of the charges for responses the invocations actually received; refusals by reason within ±max(2, 5 %); `pausedUntil` exact to 1 s; LLM credits ±2 %; fraction of fields | §4, §6 | .014 | | W |
| `t_platform_limits` | app-caused platform-limit errors per class: push per minute, per push, cyclic and payload; KVS RPS, 10 KB units and per-key 429; Realtime 50 ops/s; the bridge limiter; invocations 300/s. Per class `1/(1 + errors/5)` × "the affected work completes"; mean over classes | §10 | .016 | | M (P06 codes) + D |
| `t_economy` | mean over seeds of: background points per accepted delivery on A against the STATED reference (golden × 1.2), `min(1, ref/actual)`; KVS write units per accepted delivery against its reference; the idle final hour ≤ the stated idle allowance with 0 domain writes | §4 | .012 | | W (references stated in `RATE-MODEL.json`) |

### F.5 G — Forge LLM (0.11)

The model is a scripted fake with seeded per-case behaviour; model quality is never graded. The emulator enforces the
documented validation rules exactly (llm#16–18).

| id | measured | anchor | w | basis |
|---|---|---|---:|:-:|
| `g_model_lifecycle` | every request names a model `list()` reports `active` at call time, including after the app's OWN most-used model turns `deprecated` at H2 (graft: frontier-adversary); 0 validation 4xx from sampling parameters | §7 | .014 | M (P17) + D (`list()` statuses) |
| `g_forced_tool_validation` | a forcing `tool_choice` on the final turn; malformed arguments (wrong types, missing or extra fields, lengths) → the error state, nothing stored; JSON-string arguments accepted | §7 | .014 | D (llm#4, #5; forcing stated) |
| `g_tool_loop` | parallel tool calls answered with matching ids (the emulator answers 400 otherwise, stated); ≤ 4 rounds; unlisted tools answered with an error message and never executed; tool results drawn only from the requester's scope (a leak itself is C1 through `s_person_visibility`; this row grades the loop) | §7 | .014 | D (llm#9) + W |
| `g_grounding` | shown issue keys ⊆ keys visible to the requester (prose included); the summary as text with no element from model markup and no link; the AI label `[data-testid="ai-label"]` present | §7 | .014 | W |
| `g_failure_states` | refusal, 403 `FORGE_LLMS_MODEL_FORBIDDEN`, 429, 500, a stream ending without a non-null `finish_reason`, and an empty `end_turn` each give the stated state; ≤ 1 retry, never sooner than 30 virtual s; 60 s of silence after a 429; non-AI features keep working | §7 | .014 | D (llm#11, #12, #35–38) + W (shapes) |
| `g_placement` | drafts with 40–140 virtual s of scripted latency complete and reach the requester; 0 LLM calls during any boot; no resolver killed at 25 s while holding an LLM call (graded by outcome) | §7 | .012 | M (timeouts, P07) + D (llm#23, #25) |
| `g_dosing` | the digest: `min(1, 100/peak_rpm) × min(1, 500k/peak_tpm) × completed_fraction`, per 60 virtual s window with the stated estimator, completing ≤ 30 virtual min | §7 | .014 | D (llm#20, #21) + W |
| `g_economy` | `max_completion_tokens` on every call; an identical repeat (same scope and content) → 0 calls; an unchanged digest rerun → 0 calls; the digest on the cheapest active tier; budget exhaustion → 0 calls and the message; kill switch → 0 calls; credits in `admin.usage` within ±2 %; fraction of sub-checks (mean over seeds) | §7 | .014 | W |

### F.6 B — boot (0.07)

These are counted on cold contexts under the production CSP with the real bridge, on the virtual frame clock (§D.1 8),
by hold-and-release waves. READY is verified by seeded CONTENT. Each Custom UI row is the MEAN over board, panel and
widget (the widget measured as two instances on one dashboard).

| id | measured | anchor | w | basis |
|---|---|---|---:|:-:|
| `b_shell_first` | SHELL present at wave 0 | §5 | .008 | W + M (served form, boot#2) |
| `b_one_wave` | `min(1, 1/waves_to_READY)` | §5 | .010 | W |
| `b_ops_budget` | `min(1, budget/backend_ops_before_READY)` (invoke, requestJira, Realtime subscribe, invokeRemote, flags init) | §5 | .010 | W |
| `b_bytes_requests_depth` | mean of `min(1, KB/gz9_bytes)`, `min(1, requests_budget/requests)`, `min(1, 3/depth)`, `min(1, payload_budget/bootstrap_bytes)` | §5 | .010 | W |
| `b_bootstrap_scaling` | `min(1, rounds_budget/rounds)` × `min(1, (calls_N + pages)/calls_4N)`: two viewers whose visible candidates differ 4× | §5 | .010 | W |
| `b_csp_clean` | 0 CSP violations and 0 requests to non-app, non-platform origins (the allow-list includes the default CSP hosts, for avatars) | §5 | .006 | M (`@forge/csp`; production CSP observed, boot#7) |
| `b_no_regate` | mean of: the shell present while the Realtime subscribe is held; 0 repeated bootstrap invokes (payload equal to the boot invoke's) on filter, tab or back navigation; 0 LLM calls during boot | §5 | .004 | W |
| `b_boot_429` | the bootstrap invoke refused with 429 + `rateLimitProperties` (both field names returned, stated): the shell stays; exactly 1 retry, at or after `rateLimitReset` on the virtual clock | §5 | .006 | M (metadata shape, P14) + W (one-retry rule) |
| `b_admin_boot` | the UI Kit admin: `min(1, 1/invokes_before_first_doc_with_default_tab)` × (the first ForgeDoc is non-empty) | §6 | .006 | W |

**Excluded by design:** JS work counts and threadTicks (BRIEF §4.3 B10). No instrument for them passes the G1 double run
(boot#38).

### F.7 U — front surfaces, Custom UI function (0.07)

| id | measured | anchor | w | basis |
|---|---|---|---:|:-:|
| `u_board_numbers` | the four KPI tiles and the at-risk first page equal the oracle for three viewers (all-seeing, restricted, revoked) at CP2 and CP4 | §5 | .014 | W |
| `u_board_table` | sort by due instant and priority (stated orders), filter by priority, keyset paging with no skip or repeat while idle, issue key → bridge `router.open`/`navigate` to `/browse/<KEY>`; fraction | §5 | .008 | M (router ops, P14) |
| `u_board_live` | with the board open, a recorded change is published → the new state shows within one release wave; 0 invokes while idle (polling scores 0, 1.0 defect H) | §5 | .010 | M (Realtime pairing, P15) |
| `u_panel_timeline` | `li[data-seq]` in `seq` order; clock `time[datetime]` values equal the oracle; `not-escalation` on other issues | §5 | .008 | W |
| `u_panel_draft_flow` | request → pending → draft with the AI label → confirm → exactly one ADF comment by the viewer with the draft text; cancel → none; a failure shows `[data-testid="draft-error"]` and the panel keeps working | §5, §7 | .012 | M (ADF comment as the user, 1.0) |
| `u_quota_paused_state` | at CP3, on board, panel and widget: `quota-paused` with `datetime` = the reset instant; data shown obeys the reuse rule; bridge-fed Jira fields still shown | §4, §5 | .008 | W |
| `u_widget` | without config → `needs-config`; edit through the dashboards edit API + host Save → the view shows the chosen project's counts = the oracle for the viewer; reopening edit shows the stored choice; a second instance with another config shows its own; live update without reload | §5 | .010 | M (1.0's widget host, P14) |

### F.8 K — UI Kit admin (0.07)

Graded on the ForgeDoc tree, the host-state semantics and the bridge-call log. Pixels are never graded.

| id | measured | anchor | w | basis |
|---|---|---|---:|:-:|
| `k_admin_secret` | Integration: a valid Save → exactly one `admin.setSecret`; empty → ErrorMessage and 0 invokes; the Lozenge shows `Set` afterwards and never the value | §6 | .010 | M (Form/Textfield callbacks, P16) |
| `k_admin_roles` | a grant through the UserPicker → the `grantRole` payload's `accountId` is a STRING; revoke only after the Modal confirm; the table lists grants; the last-admin refusal message | §6 | .010 | M (UserPicker `onChange`, P16) |
| `k_admin_policy` | invalid values → per-field ErrorMessage, 0 invokes; valid → one `setPolicy` with integers and the loaded version; a double activation before the first resolves → 1 invoke | §6 | .012 | M (`useForm`/`handleSubmit`, uikit#9, #28; P16) |
| `k_admin_failures_table` | header clicks on `attempts`, `first-seen` and `age-hours` give the stated orders (the fixtures include decimal and display-date cases that mis-sort as strings); Retry → one invoke and the row updates | §6 | .010 | M (ADS comparator, en-US) |
| `k_admin_usage_migration` | the Usage and Migration tabs show the resolvers' values in the stated `testId`s; the currency column sorts by value | §6 | .008 | W |
| `k_admin_quiescence` | after boot and after each scripted action: no `reconcile` within K host ticks, no `onError`, no `BridgeAPIError` | §6 | .008 | M (the real reconciler) |
| `k_admin_budgets` | Budgets & AI: one `setBudgets` per Save; the kill-switch Toggle sends `aiEnabled: false` and afterwards 0 LLM calls happen; start-digest → one invoke | §6, §7 | .008 | W |
| `k_concurrent_admin` | two desk-admin sessions save the policy concurrently → one wins, the other shows the stated `stale` message, nothing is lost (graft: frontier-adversary's admin CAS) | §6 | .004 | W |

### F.9 V — visual polish (0.05; the owner's "pretty", made measurable)

| id | measured | anchor | w | basis |
|---|---|---|---:|:-:|
| `v_theme_tokens` | `theme.enable()` called; text colours are `--ds-text*` values; contrast ≥ 4.5:1 in both modes (disabled controls exempt) | §5 | .010 | M (pinned tokens) |
| `v_dark_surface` | the dominant painted colour of each surface is in the dark `--ds-surface*` family in dark mode and the light family in light mode; no blank surface | §5 | .008 | M |
| `v_layout_fit` | board at 1280 and 1024 px, panel at 400 px, widget at 380 px: no horizontal overflow; no `[data-metric]` number or due time clipped or ellipsised (names may ellipsise; 1.0's text-range method) | §5 | .010 | W |
| `v_states` | on stated fixtures: the loading skeleton in the shell; `empty` for a viewer with no visible escalations; `error` when the bootstrap returns an error value; `paused` during the wall; `needs-config` | §5 | .010 | W |
| `v_console_clean` | 0 console errors and page errors on nominal flows | §5 | .006 | M |
| `v_a11y` | axe-core (pinned version, shipped in the kit) serious + critical violations = 0 on board, panel and widget in both themes; focus visible on sortable headers | §5 | .006 | W (pinned tool) |

### F.10 Criticals (five; each fires only on its named consequence, OBSERVED)

| critical | fires when (observed) | factor | rows carrying the extent |
|---|---|---|---|
| **C1 data exposure** | a canary seen where it must not be, per viewer: hidden-issue CONTENT (summary, comment text, the customer of a hidden escalation); the secret outside the secret store's presence read-back; another installation's canary; a draft delivered to another person; or a hidden issue's key or id in a structured field without content (EXISTENCE) | content, secret, cross-tenant, draft: **0.6**; existence-only: **0.8** for ≤ 2 distinct (viewer, item) pairs, 0.6 for ≥ 3 | `s_person_visibility`, `s_idor`, `s_secrets`, `s_tenant_isolation`, `s_realtime_isolation` |
| **C2 duplicate side effect on the customer's Jira** | ≥ 2 comments for one note, ≥ 2 issues for one case, ≥ 2 comments for one confirmed draft, ≥ 2 breach comments for one clock breach, ≥ 2 Jira effects from one delivery id | `1 − 0.4·min(1, d/3)`, d = distinct logical events duplicated: one → 0.867, two → 0.733, three or more → 0.6 | `r_note_exactly_once`, `r_issue_exactly_once`, `r_confirm_once`, `r_scheduled_semantics`, `s_webhook_replay` |
| **C3 forged or unauthorised change** | a non-desk-admin call changed enumerated domain state (rule 13); a request that failed verification caused any domain side effect | **0.6** | `s_admin_authz`, `s_webhook_auth` |
| **C4 data loss** | an accepted case or a 1.x record absent from the CP4 export, or an applied 1.x event's effect missing | `1 − 0.4·min(1, lost/max(3, 1 % of exposed))` | `r_one_record_per_case`, `r_migration` |
| **C5 cross-tenant pool denial** | in an hour in which the pool walled outside the injected spike, the app's in-scenario background spend exceeded the stated budgets AND the counterfactual (spend minus the overspend) stays below 65,000: the app caused the wall that took every tenant down | `1 − 0.4·min(1, walled_minutes/30)` | `t_install_budget` |

**What can never fire a critical:** an absent surface (vacuous); zero comments (that is `r_note_exactly_once`'s
fraction); a missing flag or label; a harness failure (HELD); a wall the invisible installations caused alone; an
auditor or desk-admin seeing CRM-sourced fields (the carve-out); a cache or refusal-log write on a refused call
(rule 13).

### F.11 Bands and composition

| band | ceiling | requires (`passed` = exactly 1.0, available, not vacuous) |
|---|---:|---|
| 1 deployable | 0.499 | `l_deployable`, `l_bundles_load` |
| 2 core working | 0.699 | four diagnostics (weight 0): `d_intake` (≥ 50 % of valid deliveries reflected in the CP4 export), `d_export_schema` (the auditor export validates against `CONTRACT-SCHEMAS.json`), `d_board_ready` (the board reaches READY for the all-seeing viewer at CP4), `d_admin_renders` (the first ForgeDoc holds the default tab) |

**No graded robustness band** (consensus finding 1). A band step on top of a row's own loss charges one root twice,
and in five of the six panel designs it pinned the frontier to an integer count. The rows carry the severity here.

```
inner      = Σ w_row · score_row                                  (93 weighted rows, Σ w = 1.00; available rows)
crit_mult  = max(0.30, f₁ · Π_{i≥2} (1 + f_i) / 2)                 (fired critical classes, factors sorted ascending)
earned     = inner × crit_mult
final      = min(earned, ceiling − 0.05·(1 − earned))             (1.0's capped_final; ceiling 1.0 when no band fails)
```

- **Bounded compounding.** The worst critical counts in full; each further one counts at half its penalty. Two cliffs
  give 0.48 instead of 0.36, three give 0.384, and the floor is 0.30. Mid models keep visible partial credit and every
  extra critical still costs (consensus finding 2).
- **Any `unavailable` row** makes the verdict unpublishable and HELD for rescore.
- **No excellence slice.** Every efficiency target is a stated number inside a weighted row (`t_economy`, `g_economy`,
  the B budgets), so 1.0's unstated-optimum residual cannot recur.
- **Every verdict reports** final, inner (= the no-critical score), crit_mult, each fired critical with its evidence,
  failed bands, and the per-population fractions.

### F.12 ROOT_BLOCKS (attribution + multiplier dedup)

| root | blocks |
|---|---|
| `l_bundles_load` | every runtime row |
| `d_board_ready` | the board halves of B, U and V |
| `d_admin_renders` | K rows. The backend still configures through the stated admin API, so S, T and R are unaffected (security-first's cascade problem) |
| `s_person_visibility` | `g_tool_loop`'s scope sub-check when the same canary is in the tool result (the critical fires once) |
| `t_install_budget` (C5 hour) | `t_interactive_reserve` in the hour the app walled |
| P-FIELD cut | `r_field_values` removed; its weight moves to `r_jql_results` (.005) and `r_sla_clocks` (.005) |

Everything else is separated by the disjoint populations of §D.4. No other attribution is needed.

### F.13 Severity selftest and spread controls (wired into `--reference`; an inversion refuses the freeze)

**Selftest** (synthetic row sets through the REAL composition):
1. Every weighted row earns.
2. A measured, non-vacuous zero-comment row scores ABOVE one duplicate (defect B's inversion, made a test).
3. A content leak scores below a missing board, and a cross-tenant leak below a missing widget (the owner's 1.0 rule).
4. C4 with 1 of 3,000 records lost → 0.987; C2 with one duplicate → 0.867.
5. Monotonicity: raising any row never lowers the final.
6. No row has a step: for every count row, measured + 1 moves the final by less than half the row's weight.
7. The empty starter and a one-function app score ≤ 0.05.
8. An app that only answers the web trigger 202 scores ≤ 0.10.
9. Every critical fires on its named synthetic consequence and on none of the near-miss set: zero comments; an
   invisible-installation wall; a 404 to a leaked row; a refused admin call that wrote a cache entry; a desk-admin
   seeing CRM fields of an unbrowsable issue.
10. `unavailable` never lowers a score; it holds it.
11. Bounded compounding is exact: two 0.6 criticals → 0.48, three → 0.384, floor 0.30.
12. Design text = code: every partial-credit statement in this registry is asserted against the scorer (defect H).

**Spread controls** (archetype trees scored through the full scorer at the gate; their bands are part of the freeze):

| archetype | built as | must score |
|---|---|---|
| golden | — | 1.000 |
| alt A | different mechanisms (§C.12), public text only | ≥ 0.95 |
| competent but racy | golden minus conditional writes, the create anchor, the comment marker and per-person verdict scoping | 0.35–0.55 |
| naive Forge app | push per delivery, get-add-set counters, full rescans, numeric-only `Retry-After`, sleeps in functions, no budget, UI-only dedupe, asApp board | 0.10–0.30 |
| single surface | intake + export only | ≤ 0.15 |
| empty / one-function / 202-only | — | ≤ 0.05 / ≤ 0.05 / ≤ 0.10 |

Plus a **Monte Carlo of the composition** over row-failure probability tables for Sol-, Haiku-, mid- and weak-class
archetypes. It runs as a unit test and is cheap. Freeze requires P(two frontier archetypes within 0.015) < 5 % and
75th − 25th percentile ≥ 0.15 across the archetype mix (spread-judge fix).

### F.14 One-defect mutants (≥ 1 per row, ≈ 95 in all; representative set with expected losses)

| mutant | defect | expected loss |
|---|---|---|
| `m_side_effect_before_verify` | writes the inbox before checking the signature | `s_webhook_auth`, C3 |
| `m_nonce_get_then_set` | replay check by `get` then `set` | `s_webhook_replay` (concurrent pair), C2 |
| `m_nonce_ttl_short` | nonce TTL 60 s even with an `expireTime` check | `s_webhook_replay` (the 30-minute repeat) |
| `m_sign_parsed_json` | HMAC over `JSON.stringify(JSON.parse(body))` | `s_webhook_variants` |
| `m_header_case` | reads `headers['X-Desk-Signature']` only | `s_webhook_variants` (if P10 keeps case) |
| `m_admin_ui_only` | admin keys trust the page | `s_admin_authz`, C3 |
| `m_payload_identity` | `requester` taken from the payload | `s_identity_from_context` |
| `m_trust_extension` | the panel trusts `extension.issue.id` | `s_idor`, C1 |
| `m_asapp_board` | board rows read asApp, unfiltered | `s_person_visibility`, `u_board_numbers`, C1 |
| `m_verdict_forever` | access decisions cached for the session | `s_verdict_ttl` |
| `m_verdict_per_tenant` | decisions keyed by installation, not person | `s_person_visibility`, C1 |
| `m_no_verdict_cache` | a fresh check on every open | `t_person_budget`, `t_economy` |
| `m_tool_results_unscoped` | `get_comments` returns every comment asApp | `s_person_visibility`, `g_tool_loop` (shadow), C1 |
| `m_secret_echo` | `getConfig` returns the secret | `s_secrets`, C1 |
| `m_secret_kvs_set` | the secret stored with `kvs.set` | `s_secrets` (row only) |
| `m_module_cache_issue_key` | a module-scope cache keyed by issue key | `s_tenant_isolation`, C1 |
| `m_unawaited_push` | fire-and-forget push after return | `s_tenant_isolation`, C1 |
| `m_inner_html` | CRM text through `dangerouslySetInnerHTML` | `s_xss` |
| `m_execute_any_tool` | executes the model's unlisted tool call | `s_prompt_injection` |
| `m_rt_global_draft` | the draft goes on a global channel named by case id | `s_realtime_isolation`, C1 |
| `m_client_claims` | Realtime claims taken from the client | `s_realtime_isolation` |
| `m_jql_as_caller` | the function computes asUser of its first caller | `s_jql_user_agnostic` |
| `m_privacy_promise_all` | the helper's concurrent batches | `s_personal_data` |
| `m_no_erase_closed` | `closed` ignored | `s_personal_data` |
| `m_path_unchecked` | ids concatenated into paths with `assumeTrustedRoute` | `s_input_handling` |
| `m_dedupe_jobid` | idempotency on `jobId` | `r_note_exactly_once`, C2 |
| `m_last_arrival_wins` | state applied without the `seq` check | `r_monotonic_state` |
| `m_rmw_unconditional` | read-modify-write without the version condition | `r_no_lost_update` (20/20 seeds; reads-first) |
| `m_blind_comment_retry` | re-posts after a 5xx with no marker check | `r_note_exactly_once`, C2 |
| `m_search_too_soon` | ambiguous create → immediate search, no `reconcileIssues` | `r_issue_exactly_once`, C2 |
| `m_post_draft_twice` | `postDraft` not idempotent | `r_confirm_once`, C2 |
| `m_no_pause_status` | clocks ignore pause statuses | `r_sla_clocks` |
| `m_policy_retroactive` | a new policy recomputes from `openedAt` | `r_sla_clocks` |
| `m_backfill_single_invocation` | no continuation | `r_continuation` |
| `m_restart_chunk_no_watermark` | a killed chunk restarts from its start, doubling | `r_kill_safety` |
| `m_throw_on_404` | the consumer throws for a deleted issue | `r_poison_items`, `r_world_changes` |
| `m_key_not_id` | escalations keyed by issue key | `r_world_changes` |
| `m_project_delete_ignored` | no handling of project deletes | `r_world_changes` |
| `m_sweep_no_lease` | concurrent sweeps both comment | `r_scheduled_semantics`, C2 |
| `m_drop_v1_queue` | no consumer for `crm-events` | `r_migration`, C4 |
| `m_v1_due_served` | serves the 1.x `dueAt` during migration | `r_migration` |
| `m_keep_v1_keys` | never deletes `esc1:` | `r_migration` |
| `m_no_ignore_self` | reacts to its own updates | `r_self_events` |
| `m_global_serialize` | one consumer, `limit: 1`, reading Jira per delivery | `r_freshness` |
| `m_push_per_delivery` | one push per delivery, `RateLimitError` unhandled | `t_platform_limits`, `r_freshness` (+ C4 if work is lost) |
| `m_throw_on_5xx_resolver` | resolvers throw on an upstream 5xx | `r_resolver_contract` |
| `m_field_per_issue` | one `/app/field/value` request per issue | `t_economy` |
| `m_field_stale` | no field update on time-driven transitions | `r_field_values` |
| `m_jql_id_list` | the fragment is an id list | `r_jql_results` (> 1,000) |
| `m_no_accounting` | no background budget | `t_install_budget`, C5 (burst hour) |
| `m_pause_endpoint_on_quota` | a quota 429 pauses only its endpoint | `t_pool_wall_pause` |
| `m_ignore_r` | does not read `RateLimit` | `t_near_limit_backoff` |
| `m_retry_after_numeric` | `Number(retry-after) \|\| 1` | `t_pool_wall_pause`, `t_retry_discipline` (gateway variant) |
| `m_sleep_in_function` | waits out `Retry-After` in the invocation | `t_retry_discipline` (b), `r_continuation` |
| `m_promise_all_pages` | unbounded `Promise.all` | `t_in_flight` |
| `m_top_of_hour` | the scheduled job does everything at :00 | `t_smoothness` |
| `m_flat_points` | counts 1 point per call | `t_usage_accuracy`, `t_install_budget` |
| `m_hot_counter_key` | one KVS key for all accounting | `t_platform_limits` (per-key), `t_usage_accuracy` |
| `m_parallel_notes` | posts a hot case's notes in parallel | `t_per_issue_writes` |
| `m_no_user_throttle` | the masher's requests all served | `t_person_budget` |
| `m_await_before_render` | `await invoke()` before `render()` | `b_shell_first` |
| `m_waterfall` | getContext → invoke(config) → invoke(data) | `b_one_wave`, `b_ops_budget` |
| `m_dev_react` | React development build | `b_bytes_requests_depth` |
| `m_n_plus_1_bootstrap` | one Jira read per row in the bootstrap | `b_bootstrap_scaling` |
| `m_cdn_font` | Google Fonts in `index.html` | `b_csp_clean` |
| `m_reinvoke_on_filter` | bootstrap again on a filter change | `b_no_regate` |
| `m_retry_storm_429` | three immediate bootstrap retries | `b_boot_429` |
| `m_admin_three_invokes` | three invokes before the first tab | `b_admin_boot` |
| `m_poll_board` | polls every 5 s instead of Realtime | `u_board_live` (0) |
| `m_post_on_arrival` | posts the draft without confirmation | `u_panel_draft_flow`, `s_prompt_injection` |
| `m_widget_global_config` | the widget reads its config from storage, not `extension.config` | `u_widget` |
| `m_paused_blank` | blank surfaces during the wall | `u_quota_paused_state` |
| `m_display_date_key` | failures keyed by display dates | `k_admin_failures_table` |
| `m_userpicker_object` | sends the UserPicker object | `k_admin_roles` |
| `m_no_handle_submit` | `onSubmit` without `handleSubmit` | `k_admin_policy` |
| `m_policy_no_version` | saves without the version | `k_concurrent_admin` |
| `m_setstate_storm` | a `setState` after every `await` | `k_admin_quiescence` |
| `m_hardcoded_white` | no tokens | `v_theme_tokens`, `v_dark_surface` |
| `m_overflow_380` | a fixed-width widget table | `v_layout_fit` |
| `m_no_empty_state` | a blank page for a viewer with nothing visible | `v_states` |
| `m_unlabelled_buttons` | icon buttons without labels | `v_a11y` |
| `m_temperature_top_p` | the README's sampling parameters | `g_model_lifecycle`, `g_failure_states` |
| `m_stale_model` | the model chosen at first call and kept | `g_model_lifecycle` |
| `m_trust_args` | no argument validation | `g_forced_tool_validation` |
| `m_first_tool_call_only` | answers only `tool_calls[0]` | `g_tool_loop` |
| `m_trust_cited_keys` | shows every cited key | `g_grounding` |
| `m_no_truncation_check` | a stream with no `finish_reason` shown as complete | `g_failure_states` |
| `m_llm_in_resolver` | drafts inside the resolver | `g_placement` |
| `m_digest_unpaced` | `Promise.all` over the digest | `g_dosing` |
| `m_no_llm_cache` | no cache | `g_economy` |
| `m_rovo_payload_identity` | the action trusts an `accountId` input | `s_rovo_inputs` |
| `m_skill_name_mismatch` | SKILL.md `name` ≠ its directory | `l_manifest_contract` |

Every mutant carries `{loses: [...], critical, max_final}`, written BEFORE scoring (never from scorer output).
Platform-semantics mutants (`m_rmw_unconditional`, `m_nonce_get_then_set`, `m_search_too_soon`) must also misbehave on
real Forge (§I.4).

### F.15 The mandate checklist → where it is graded

| # | mandate item (NOW.md) | stated in | graded by |
|---|---|---|---|
| 1 | latest Forge modules combined | §B; contract §0, §8 | `l_manifest_contract` plus each module's functional rows: `u_widget`, `r_field_values`, `r_jql_results`, G (Forge LLM GA), `u_board_live` (Realtime tokens), K (UI Kit 2), `r_no_lost_update` (CES conditions), `b_boot_429` (`rateLimitProperties`), S webhook rows (static trigger), `s_rovo_inputs` |
| 2 | big, tough app | §A–§D | 93 rows over 3 installations and 6 virtual hours; Sol 0.40–0.75 (§G) |
| 3 | complex backend resolvers | §C.3; contract §2, §6 | `s_admin_authz`, `s_idor`, `s_input_handling`, `r_resolver_contract`, `r_confirm_once`, `t_person_budget`, `b_bootstrap_scaling`, `s_verdict_ttl` |
| 4 | Forge security concepts | contract §2, §3, §11 | S tier (17 rows) + C1, C3 |
| 5 | stability and robustness | contract §1, §9, §10 | R tier (19 rows) + C2, C4 |
| 6 | Custom UI boot speed | contract §5 | B tier on three surfaces + `b_admin_boot` |
| 7 | front-facing Custom UI, pretty | contract §5 | U + V tiers; screenshots and video published |
| 8 | UI Kit admin panel | contract §6 | K tier + `b_admin_boot` + `d_admin_renders` |
| 9 | a ton of calls, dosed within Tier 1, robust under bursts | contract §4, §10 | T tier (13 rows) + C5 |
| 10 | good Forge LLM usage | contract §7 | G tier + `s_prompt_injection` + `s_person_visibility` (prompt scope) |
| 11 | judicious checking (1.0 §17.8) | §F.0, §F.13, §I | the rules, selftest, spread controls, mutants, gates G1–G14 |
| 12 | PROOF that Opus 5.5 / GPT-6.1 Sol are challenged | §G.5 | the phase-4 gate (pre-build, numeric bar), Pilot-0, and the phase-7 acceptance table |

---------------------------------------------------------------------------------------------------------------------

## G. Difficulty argument (GPT-6.1 Sol and Opus 5.5)

Caveat carried from the source: the Sol, Opus 5.5 and Sol Pro trees are not on this machine. Statements about HOW they
work are inferred from Haiku 5.5 (0.9661) and Pareto (0.9450), which sit 0.0108 and 0.0319 below them, and from their
board rows (frontier-behaviour §6: MEDIUM confidence). Phase 4, Pilot-0 and the phase-7 pilot settle the predictions.

### G.1 How the frontier won 1.0, and what 2.0 removes

| how they won 1.0 (quoted) | what 2.0 does instead |
|---|---|
| "The contract works as a test plan. Each injected fault is announced" | GUARANTEES, platform FACTS and the benchmark's WORLD only (§D.5); no timing, targets or order; no "the scheduled job runs first" |
| "Every semantic trap … traces to a contract sentence that Haiku paraphrased" | paraphrasing "exactly one comment per note, whatever happens between your app and Jira" does not produce the marker-then-check protocol; populations keyed to entities make every protocol hole cost its row |
| "All six defect classes Haiku found by testing were platform wiring, each announced by an explicit dev-tool error string" | dev tools print only production messages; a race, a duplicate or a leak is silent, so only the app's own checks find it |
| "The scoring world is small and static … measured overlap: 0 … the longest Retry-After was 30 seconds" | three installations, up to 8,000 issues, a burst above the push limit, interleaved invocations under a reads-first schedule, a 25–45-minute wall against a 900 s `retryAfter` ceiling, a world that changes mid-run |
| "The budget never bound" (72 and 50 of 150 calls) | 300 calls: depth must be engineered, not raced |
| the residual was an unstated optimum (`e_event_economy` 0.40–0.42 for the top five) | every target is a stated number; losing points means failing a stated requirement |
| Haiku "never exercised the restricted viewer … and it scored 1.0" | per-viewer canaries over every channel, including LLM tool results, under a stated threat model |

### G.2 The mechanisms Sol most plausibly gets wrong (P = my estimate that the row loses ≥ 30 % of its value)

| # | mechanism (rows) | why plausible (frontier-behaviour quotes) | P |
|---|---|---|---:|
| 1 | lost updates under concurrent delivery with a throughput bar (`r_no_lost_update`, `r_freshness`) | Haiku: "I'm accepting the eventual consistency tradeoff here since concurrent writes both read fresh state anyway". Pareto's `limit: 1` site-wide serialisation now misses the 10-minute bar once it reads Jira per delivery; reads-first exposes every read-modify-write | 0.50 |
| 2 | exactly-once issue creation (`r_issue_exactly_once`, C2) | lost answer + stated search freshness + concurrent first deliveries; "search right after the create" is the natural code | 0.55 |
| 3 | exactly-once comments (`r_note_exactly_once`, C2) | Pareto: "Never retry a successful POST; the UI coalesces a double click"; Haiku: "rather than adding backend-side deduplication". Here writes happen in redelivered consumers | 0.45 |
| 4 | intake above the push limit (`t_platform_limits`, `r_freshness`, possibly C4) | one push per delivery is the canonical Forge pattern, and 1.0's own golden; at > 500 events/min it fails or loses work | 0.50 |
| 5 | a shared pool the app cannot see (`t_install_budget`, `t_near_limit_backoff`, `t_pool_wall_pause`, C5) | models budget what they can count; `r` appears only past 80 %; the gateway 429 carries no `Retry-After` | 0.50 |
| 6 | a wall longer than the 900 s clamp; no in-function waits (`t_retry_discipline`, `r_continuation`) | Haiku: "which works fine since consumer invocations can run up to 900 seconds"; `Number(…'retry-after') \|\| 1` | 0.45 |
| 7 | security against Tier 1: decision reuse keyed per (person, issue) ≤ 5 min under a per-person cap (`t_person_budget`, `s_verdict_ttl`, `s_person_visibility`) | Haiku and Pareto computed visibility per request "with no cache to go stale"; here that breaks the cap, and the doc's own "SAFE" cache is keyed per tenant (sec#29) | 0.45 |
| 8 | prompt scope through tool results with role-restricted comments; Realtime to the requester only (`s_person_visibility` C1, `g_tool_loop`, `s_realtime_isolation`) | Atlassian's own LLM + Realtime tutorial is exploitable (llm "newer facts" 5); "browse = see every comment" is the natural assumption, now stated false | 0.45 |
| 9 | live 1.x migration (`r_migration`, C4) | "Neither app versions its storage"; key → id with moved issues, queued 1.x events, `migrating` vs a wrong number, deleting keys only after | 0.50 |
| 10 | a world that changes mid-run (`r_world_changes`, `r_sla_clocks`) | "Pareto's consumer throws on a 404 … redelivered for 24 hours"; "both store issue keys, which go stale after a move"; a project delete emits no child events | 0.50 |
| 11 | the JQL function at scale (`r_jql_results`, `s_jql_user_agnostic`) | an id list computed on call overflows above 1,000 and goes stale, because Jira does not call again | 0.50 |
| 12 | scheduled anomalies (`r_scheduled_semantics`, C2) | "expect at least one" duplicate (rob#40); a breach comment posted by two overlapping sweeps | 0.35 |
| 13 | model lifecycle (`g_model_lifecycle`) | choose once and cache; the emulator deprecates the model the app itself used most | 0.35 |
| 14 | boot discipline on three surfaces (`b_one_wave`, `b_ops_budget`, `b_bootstrap_scaling`) | the natural shape is getContext → config → data; a frontier model that reads the numbers may pass | 0.35 |

Where Sol and Opus should sit near 1.0: L, admin authorization, HMAC basics, secrets, XSS, theme, most of the UI Kit
wiring, the LLM's basic validation path.

### G.3 Predicted scores

**Sol tier walk** (my judgment, not a measurement):

| tier | L | S | R | T | G | B | U | K | V |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| expected tier score | .95 | .80 | .63 | .60 | .70 | .75 | .80 | .75 | .85 |
| weighted | .0475 | .144 | .139 | .108 | .077 | .0525 | .056 | .0525 | .0425 |

Inner ≈ 0.72; with no critical, the plausible range is 0.66–0.78.

**Criticals** (my estimates): C2 P ≈ 0.5 (an extent-scaled duplicate is more likely than a full cliff); C1 P ≈ 0.35
(tool-result scope, Realtime, a per-tenant cache); C4 P ≈ 0.15, mostly small extent; C5 P ≈ 0.1; C3 P ≈ 0.05.

| outcome | P (judgment) | final |
|---|---:|---:|
| no critical | ≈ 0.30 | 0.66–0.78 |
| one extent-scaled C2 (one or two duplicates) | ≈ 0.25 | 0.50–0.66 |
| one cliff-class critical (a content leak, C3, or C2 at ≥ 3) | ≈ 0.25 | 0.40–0.47 |
| two | ≈ 0.20 | 0.32–0.38 |

**Predicted GPT-6.1 Sol: 0.40–0.75 for about 80 % of outcomes, central ≈ 0.55**; P(< 0.45) ≈ 0.3. Every one of those
losses traces to a stated requirement.

| entrant | predicted final |
|---|---|
| Opus 5.5 | 0.40–0.78 (slower and more thorough on 1.0, so it may test more) |
| Haiku 5.5 class | 0.25–0.50 |
| mid models (1.0's 0.75–0.85 tier) | 0.12–0.35 (inner ≈ 0.40–0.55; 1–2 criticals at bounded compounding) |
| weak | 0.00–0.15 (band 1, vacuity) |
| reference / alt A | 1.000 / ≥ 0.95 (gates) |

The spread is gated by the archetype controls and the composition Monte Carlo (§F.13), not asserted.

### G.4 Why this is engineering judgment, not trivia or volume

- **Not volume.** frontier-behaviour §5.3: "Pure volume … would not challenge them". Three Custom UI surfaces, one UI
  Kit page; 0.58 of inner sits in R, T and S, where the hard part is a protocol, not a file count.
- **Not trivia.** The documented knowledge traps carry small weight and each has a dev-site signal:
  - sampling parameters (part of `g_model_lifecycle`);
  - the one-`globalPage` rule and the server lint rules (part of `l_deployable`);
  - the UI Kit callback shapes (part of the K rows);
  - `ForgeLlmAPIError` (part of `g_failure_states`).
- **Not hidden preferences.** Every budget, objective and economy reference is a number in the contract.
- **What remains is judgment.** Partitioning or conditional writes; where an idempotency key lives and when it is safe
  to search; which reads belong in the browser (exempt), the resolver (charged to a person) or the background
  (budgeted); how to chain past a wall longer than `retryAfter`; how to migrate while traffic flows; how to cache access
  decisions safely; how to scope an LLM's tool results. None of these can be transcribed from a sentence, every one
  has a measurable right answer, and every hard row has at least two valid designs (§C.12).

### G.5 The proof plan (NOW.md phases 4 and 7), stated in advance

**Phase 4 — the pre-build difficulty gate.** It runs on the public text alone and gates WP7.
1. Three fresh planners get only the public files plus read-only typings, schema and OpenAPI: GPT-6.1 Sol, Opus 5.5,
   and Opus 5.5 at high effort. Each writes a full design and test plan. Cost ≈ $1–3 per plan.
2. Blind graders (separate agents holding the hidden registry) score each plan against the 14 mechanisms of §G.2 and
   the five critical classes. Every miss is classified DERIVABLE (citing the contract sentence) or NOT DERIVABLE.
3. **Pass bar:** the median plan misses or mis-designs ≥ 4 of the 14 derivable mechanisms, and every not-derivable
   miss is fixed in the contract text before the build.
4. If a planner covers ≥ 12 of 14, harden the couplings (burst size, contended cases per seed, the wall length, the
   invisible draw), never the trivia, and repeat.
5. The planners' reading calls are recorded as the desk-audit measurement for the contract size.

**Pilot-0** (an owner decision on cost, §K). One Sol run from the Benchmark view (gate 3) as soon as the backend lanes
score (≈ day 12–16), on the then-current text, scored on L, R, T and the backend S rows. It is NOT the acceptance test.
It tells us where Sol loses before the UI lanes and the gate are spent.

**Phase 7 — the acceptance test**: one GPT-6.1 Sol run on the frozen 2.0, read as follows.

| verdict | condition |
|---|---|
| **challenged** (accept) | final ≤ 0.80 AND inner ≤ 0.85 AND ≥ 0.10 of inner lost on rows anchored to contract sentences in ≥ 2 of R, T, S, G, each loss confirmed by an independent reader of the primary logs as an app defect against the quoted sentence (gate 7); zero `unavailable` rows; reference 1.000 and alt A ≥ 0.95 on the same scorer build |
| **cruise** (harden before anyone else runs) | final ≥ 0.85, or < 0.05 of inner lost outside a single critical. Hardening levers, all stated changes: burst size and rate, contended cases per seed, the wall length, the hot hour's draw, the AMBIG population sizes, freshness windows, the per-person cap |
| **too hard or harness-suspect** (audit before calling it challenged) | final ≤ 0.30, or ≥ 25 % of lost inner classed by the independent reader as interpretation or harness |
| **ambiguous** | 0.80 < final < 0.85, or a single critical decides between "challenged" and "cruise": report the no-critical inner beside the final; a second Sol run is an owner option (≈ $6.5) |

---------------------------------------------------------------------------------------------------------------------

## H. Fairness: how a capable model can know every graded thing

1. **Every row has an anchor, including its numbers.** `test_score_forge2.py` refuses a row without an anchor, an
   anchor without a row, and any numeric threshold not present verbatim in its sentence or `RATE-MODEL.json` (§F.0
   rule 9). The 1.0 mechanism checked sections; this one checks numbers, which is where all six panel designs failed.
2. **Every number is written down:** budgets, caps, windows, the reuse bound, freshness objectives, boot numbers, the
   pool and its inequality, the cost table, bucket refill and capacity, the latency model, the token estimator, credit
   rates, the economy references. A model can compute whether a design meets them before writing code; the latency
   model is stated precisely so that throughput is a calculation, not a guess.
3. **Every platform behaviour the scorer exercises is a documented fact named in §E §10**, with its URL. Where docs
   conflict, the contract pins one side (BRIEF §9.1's 31 pins) and the scorer never grades the ambiguous side.
4. **Every behaviour class occurs at least three times on the dev world**, including the permission-rich world, the
   wall, the gateway 429, the burst above the push limit, lost answers, terminations and the deprecation. WP1 proves it
   from class-tagged events (gate G10). `--scale upper` lets a model verify at the envelope.
5. **Dev messages are production messages** (measured), and every grader instrument has a dev twin (`ledger`,
   `boot report`, `uikit`, `export validate`). The dev world teaches what production teaches and nothing more.
6. **No named fixtures and no scripted sequences** to transcribe. 1.0's "the scheduled job runs before any update" is
   gone; in its place stands a fact: "events may arrive before your first scheduled run".
7. **Outcome grading accepts every correct design** (§C.12). Alt A proves it at ≥ 0.95. Alt B (blind, another model
   family, cheap) classifies every loss as "app defect, citing the broken sentence" or "contract gap", and every gap is
   fixed in text. A fresh agent rebuilds alt B; it is never patched against grader output.
8. **Faults are keyed to entities**, so every implementation meets the same adversity whatever its call order. The
   deprecation targets the app's own most-used model.
9. **Partial credit everywhere, harness faults held, host chrome outside the frame**, and a two-verdict precondition
   column, so the robust design is never charged for being robust.
10. **Knowledge traps only where Atlassian documents the rule and the emulator enforces it exactly** (the 1.0
    `temperature`/`top_p` precedent), each small and each with a dev-site signal.
11. **No unstated harness choice decides a row.** Every emulator behaviour is M, D or W, taint-tracked, and a G refuses
    the freeze (§F.0 rule 8).
12. **The threat model is stated** ("assume every user knows every channel name and every resolver key"), so the
    security rows test reasoning from a stated model, not knowledge of the attack list.

---------------------------------------------------------------------------------------------------------------------

## I. Fidelity plan

### I.1 Layers (real-forge-fidelity §4, adopted as the build's spine)

| layer | what | when |
|---|---|---|
| **L0** lint | offline: the CLI's client half (byte-identical pins), the pinned deprecated-runtimes flag, the measured server-rule pack, a CLI-equal file walk (`.mjs`/`.cjs`, UI Kit resource directories, `.gitignore`), and every finding with its provenance (`client` / `server-measured <date>` / `docs-only (not charged)`) | every score; the corpus is re-run before each freeze and by a weekly drift job (online, ours only) |
| **L1** conformance | the SAME probe source runs live on wolfaenpak and in the emulator; the live answers are committed as fixtures (date, CLI, runtime, invocation ids); differences fail the kit test under per-probe comparator classes (§I.6) | before each freeze; the emulator replay runs in seconds |
| **L2** real deploy | (a) golden gate: the golden and alt A deploy, install and pass a scripted smoke test on wolfaenpak; platform-semantics mutants must misbehave live. (b) audit lane: each published baseline's final tree is deployed (no install) to an audit app and its accept or refuse compared with the scorer's L verdict; a mismatch blocks publication | freeze; per published baseline |
| **L3** browser calibration | the boot-lane ranking (golden ahead of `m_waterfall` and `m_dev_react`, as real Jira's time-to-READY ranks them); Realtime delivery and pairing; UI Kit callbacks | quiet windows only; never during scoring or a paid run |

Nothing on real Forge is ever a score input: a desktop user's machine has no Atlassian credentials. Rate limits are
never probed on a cloud tenant (tiers#48); header grammar is observed passively on ordinary calls.

### I.2 Every emulated behaviour → its probe, class, gate and rows

| probe | behaviour | class | gates | rows resting on it |
|---|---|:-:|---|---|
| P01 | KVS/CES codes, atomicity, 25-op and 25-key limits. **Exists**: 13/24 match today; align the 11 diffs (409 `KEY_CONFLICT`, 400 `CONDITIONAL_CHECK_FAILED`, 422 `UNPROCESSABLE_ENTITY`, `KEY_DUPLICATION_ERROR`, `TOO_MANY_BATCH_ENTITIES`, 404 `SCHEMA_NOT_FOUND`, `INCORRECT_PROPERTY_TYPE`) | M | contract + golden | every storage path |
| P02 | `query`: default 10 / max 100 / the 101 error, `beginsWith`, CES `where`/`sort`, cursor scope across invocations, read-after-write lag | M (the lag is then stated ≥ the observed p99) | contract | `r_continuation`, `r_migration` |
| P03 | TTL expiry granularity; expired-but-readable (`EXPIRE_TIME` metadata); TTL in the 4th argument of `transact().set`; **writes**: `FAIL_IF_EXISTS` and a CES condition against an expired-but-unpurged key; entity `set` with `FAIL_IF_EXISTS`; a condition on a missing entity (N5) | M | contract + golden | `s_webhook_replay`, `s_verdict_ttl`, leases |
| P04 | conditional transactions under 50 concurrent consumers; 50 concurrent `FAIL_IF_EXISTS` claims on one key (exactly one wins) | M | golden | `r_no_lost_update`, `s_webhook_replay` (and the racy mutants live, §I.4) |
| P05 | secret store round trip; secrets invisible to `query` | M | — | `s_secrets` |
| P06 | async: redelivery schedule after a throw and a timeout; `retryReason` values (the 1.0 emulator's `FUNCTION_ERROR` is suspect against `FUNCTION_TIME_OUT`); the `retryAfter` clamp; 4 KB `retryData`; concurrency-key overlap; duplicates and ordering as observed; `delayInSeconds` accuracy; push limits (51 events, 201 KB, 501 per minute) and their error classes; 100 KB per event for long consumers; cyclic counting (N10) | M (codes, limits) + D (24 h retention, "no ordering") | contract | `r_poison_items`, `r_kill_safety`, `t_platform_limits` |
| P07 | timeouts per module; partial writes persist after a kill; a request in flight at the kill; `invocationRemainingTimeInMillis` | M | contract | `r_continuation`, `r_kill_safety`, `g_placement` |
| P08 / P09 | warm reuse within and across installations; unawaited work after return | M if observed, else D (rob#11, #12) | — | `s_tenant_isolation` (graded only on crossings the stated reuse policy creates) |
| P10 | web trigger: raw `body` bytes for non-canonical JSON; header-name case and multi-value delivery; static `outputs` incl. 503; `webTrigger.getUrl` form; 424 after uninstall. **Partly done** (request fields, 424) | M | contract (header case decides `s_webhook_variants`) | S webhook rows, `k_admin_secret` (URL) |
| P11 | product events: `updated` with status, priority and move items; `deleted:issue`; `deleted:project` (no per-issue deletes); `selfGenerated` with and without `ignoreSelf`; `installed`/`upgraded` payloads; observed delay; changelog `created` against event time | M (shapes) + D (3-minute bound, rob#33) | contract | `r_sla_clocks`, `r_world_changes`, `r_self_events`, `r_migration` |
| P12 | scheduled: first tick ≈ 5 min after deploy; `{statusCode: 204}`; a throw is not retried | M (first tick, throw) + D (duplicates, rob#40) | — | `r_scheduled_semantics` |
| P13 | Jira REST behind the protocols: comment `properties` on create and read back through `POST /comment/list?expand=properties`; issue create with properties and labels; `search/jql` freshness after create, with and without `reconcileIssues`; GET by an old key after a move; 404 after delete; `permissions/check` (incl. `globalPermissions: ADMINISTER`; whether an asApp check for another `accountId` needs ADMINISTER, per the OAS clause); `mypermissions`; asApp vs asUser on a security-level issue, a reporter-only project and a ROLE-RESTRICTED comment; bulkfetch `issueErrors`; passive rate-limit header grammar on ordinary responses | M | contract + golden | `r_note_exactly_once`, `r_issue_exactly_once`, `r_world_changes`, `s_person_visibility`, `s_admin_authz`, T header grammar |
| P14 | the Custom UI bridge on real Jira (browser): served form, `requestJira` header forwarding (FRGE-1923), `rateLimitProperties` field names, `getContext` for globalPage, issuePanel and widget, router ops, `theme.enable` | M | L3 | B rows, `u_board_table`, `b_boot_429`, `u_widget` |
| P15 | Realtime: `publish` vs `publishGlobal` from a resolver and a consumer; `subscribe` vs `subscribeGlobal` pairing; claims filtering; subscribe-only and publish-only tokens; the context-scoped default; `errors[]` without a throw | M | contract text = the probe result verbatim (1.0 defect D cannot recur) | `u_board_live`, `s_realtime_isolation`, `u_panel_draft_flow` |
| P16 | the UI Kit host: callback arguments (Form `onSubmit`, Tabs `onChange`, UserPicker `onChange`, Textfield `number`/`password` events, Checkbox, Toggle), DynamicTable sort by `key` on decimal, date and currency fixtures, `testId`s delivered, double-activation timing, ForgeDoc captured through an app-side `callBridge` spy | M (callbacks, sort) + D (component semantics from the 12.3.0 source) | host fixtures | K rows, `b_admin_boot` |
| P17 | Forge LLM: validation (`temperature` + `top_p`; parameters on opus-class models), `list()` statuses, streamed tool-call chunking, parallel tool calls, `finish_reason` values, `usage`, the unknown-model error. Cents of tokens; confirm the `llm` module is enabled for the space first | M + W (refusal and moderation shapes stated) | contract | G rows |
| P18 / P18b | JQL function: invocation on search, precomputation reuse, the same fragment for two users, the computation update API, > 1,000 values (the exact error), a 26 s handler / `jira:entityProperty` usable in JQL | M | contract | `r_jql_results`, `s_jql_user_agnostic` |
| **P-FIELD** | `jira:customField` (`number`, `readOnly`): `POST /app/field/value` single and bulk (ids per entry), `cf[]` search with `=` and `in`, index lag after writes, `generateAppEvents`, errors for deleted issues | M | **the field rows' cut gate** | `r_field_values`, `t_economy` |
| P19 | lint and deploy corpus (≈ 40 manifests): every 2.0 module and storage feature, two `jira:globalPage`, a UI Kit resource directory, static-trigger outputs, consumer `function`/`resolver` forms, `timeoutSeconds` placement, index names and ranges, > 5 scheduled / 2 `fiveMinute`, `unsafe-*`, runtimes; `forge deploy --no-verify` legs (deploy-time refusals); `forge eligibility` on the golden; deploy-test v04–v06 | M | contract + golden | `l_deployable`, `l_roa_eligible` |
| P20 | Rovo action context (`accountId` from context, input coercion), if Rovo is enabled on wolfaenpak | M if available, else D (sec#26) | — | `s_rovo_inputs`, `s_identity_from_context` |
| P21 | the install race: 401/403 on early asApp calls | M if reproduced, else D (rob#37; CHANGE-3445 is rolling out) with the stated window | — | `r_install_race` (.004) |
| P22 | privacy API with the documented test ids (Active `5be24ad8b1653240376955d2`, Closed `5be24ba3f91c106033269289`); the 429 shape | M (+ W one-in-flight) | contract | `s_personal_data` |
| P23 | `asUser()` in queued work a person started | M | the stated tool-principal rule | `g_tool_loop` (design freedom, §C.12) |
| P24 | latency receipts per endpoint class at low volume (+ push-to-start) | M (order of magnitude) | the W latency model | `r_freshness`, `r_continuation` (calibration) |
| P25 | bundler parity: a `.jsx` without `import React` (classic pragma), `.mjs`/`.cjs`, JSON imports, through real `forge deploy` | M | `l_bundles_load` | `l_bundles_load` |
| P-ENTRY | bridge injection into named Custom UI entries | M | allows multi-entry | none until measured |

**Sequencing.** P01, P03, P04, P10, P11, P13, P15, P16, P18 and P-FIELD are CONTRACT-GATING and GOLDEN-GATING: the
contract freezes and the golden is written only after their receipts (WP0 → WP7 dependency; fidelity-judge fix).
Total live work ≈ 3–4 agent-days to build plus ≈ 6 h of mostly idle wall per calibration pass, CPU < 5 min.
Every session runs at `nice -n 19`, one command at a time, never while a paid run is scoring; browser legs only in quiet
windows. Probes are uninstalled and fixtures restored afterwards (wolfaenpak is the sanctioned test site).

### I.3 Lint fidelity (L0)

The kit's `npm run lint` has four parts:
1. the CLI's client half, unchanged (`@forge/lint` 6.3.0, `@forge/manifest` 13.6.0, `@forge/cli-shared` 9.7.0,
   byte-identical to CLI 14.1.0);
2. the measured server-rule pack: v01 two range attributes, v02 an index name too short, v03 not-allowed characters,
   v07 > 20 entities, v08 `nodejs20.x`, v10 an index name too long — extended by P19 to every 2.0 feature;
3. the pinned flag value `["sandbox","nodejs18.x","nodejs20.x"]`;
4. a CLI-equal file walk.

Only measured rules can fail `l_deployable`. Docs-only constraints (v04–v06 until deploy-tested) are reported and never
charged.

### I.4 Real Forge as a gate, never a score input (L2, L3)

- **The golden gate at freeze.** Deploy and install on wolfaenpak; run a scripted smoke test:
  - a signed CRM POST becomes an escalation and an issue;
  - a note becomes a comment;
  - the board, panel and widget load with data;
  - the admin page saves a policy;
  - `escalationBreached()` answers in a JQL search;
  - the field shows a value;
  - `forge eligibility` reports RoA-eligible.

  Then uninstall and delete the created issues.
- **Platform-semantics mutants live.** `m_rmw_unconditional` under P04's parallel burst must lose updates on real Forge
  in at least 1 of 3 trials at the stated stimulus scale. `m_nonce_get_then_set` must double-apply on a concurrent
  live pair. `m_search_too_soon` must create twice after a live create. A mutant with no live misbehaviour in 3 trials
  is dropped: it would measure the emulator, not Forge.
- **The audit lane** for published baselines (L2b) and the **browser calibrations** (L3) in quiet windows.

### I.5 What stays unmeasured, and how the design avoids grading a guess

| unmeasured | why | handling |
|---|---|---|
| Tier 1 pool behaviour, forgiveness, bucket capacity, per-endpoint costs, other apps' traffic | "Do not perform rate limit testing against Atlassian cloud tenants" (tiers#48) | W: `RATE-MODEL.json` owns every value and says it is the benchmark's world; only the header GRAMMAR rests on docs and passive observation |
| real latencies | variable; not a platform contract | W latency model, its order of magnitude checked by P24; objectives set at ≥ 2× the golden's worst seed |
| 24 h and 96 h retention; the 3-minute delay bound; scheduled duplicates, skips and overlaps | too long or not forceable live | D: doc quote + stated emulator choice; a correct app passes for any value inside the envelope |
| `query` lag magnitude | "slightly out of date" only | D with the stated lag ≥ P02's observed p99; outcomes a get-based design passes for any lag |
| maximum concurrency keys and limits | undocumented | the emulator imposes no maximum; no row depends on one |
| LLM refusal, moderation and 429 shapes | not forceable cheaply | W shapes stated; `g_failure_states` accepts any `ForgeLlmAPIError` status for "error" (1.0 precedent) |
| UI Kit product-host internals | the renderer is closed (uikit#13) | allowlist + `unmodelled` marker; P16-measured callbacks only; never pixels |
| asApp visibility of security levels | sec#10 outdated | stated worst case (asApp sees everything); P13 records the live behaviour; a correct app passes either way |
| constant-time comparison; CPU and memory timing | not observable deterministically | not graded; `memoryMB` honoured as a heap cap only |

### I.6 Comparator classes and taint tracking

- **Comparator class per probe case** (fidelity-judge fix), since "any difference fails" cannot hold for
  nondeterministic behaviour:
  - **exact** for codes, shapes and atomicity;
  - **envelope** for timing and retry intervals: the emulator's worst case ⊇ the live p99;
  - **existential** for duplicates, overlap and reorder: they can occur in both;
  - **never-contradicts** for documented-only behaviour.
- **Taint tracking.** Every emulator behaviour site is tagged M, D, W or G. Each verdict records the tags its rows'
  evidence passed through. Any G refuses the freeze (gate G7).

---------------------------------------------------------------------------------------------------------------------

## J. Build plan

### J.1 Work packages (one owner per file; 2.0 lives in its own tree per integration D1; agent-days describe blast radius, not priority)

| WP | owns | contents | agent-days | depends on |
|---|---|---|---:|---|
| WP0 | `forge2/probe/**`, fixtures, `FIDELITY.md` | P01–P25, P-FIELD, P-ENTRY; the conformance harness with comparator classes; the lint corpus | 4–5 (+ ≈ 6 h live wall, mostly idle) | — |
| WP1 | `forge2/kit/lib/{runtime,runner,scheduler,proxy,agent}` | the lockstep core: warm workers, the agent (timers, clocks, entropy, `lambdaContext`), scheduler (canonical + reads-first), kill by pid, platform terminations, freeze-at-return, CPU rule, harness heartbeat, checkpoint freeze, operations bound, determinism tests. **First task: the go/no-go spike (§J.2)** | 7–9 | — |
| WP2 | `forge2/site/**` | mock Jira 2.0: mutable world and timeline (the 31 `pack.*` reads routed through state), security levels, reporter-only projects, role-restricted comments, moves, deletes, project deletes, comment properties and `comment/list`, issue properties and labels, search freshness + `reconcileIssues`, bulkfetch, the app-field store and `cf[]` search, JQL function substitution with a precomputation cache; the points gateway (pool, invisible draw, cost table, buckets, per-issue windows, headers incl. the gateway variant, ambiguous writes); the scale generator with populations (N3 fixed) | 10–13 | WP0 (P11, P13, P18, P-FIELD) |
| WP3 | `forge2/kit/lib/{emulator,kvs,realtime,llm,webtrigger,privacy}` | async (keys, limits, push failures, cyclic, retries, jobs, 1.x bodies, every consumer form); KVS aligned (P01–P04) + units, RPS and per-key limits; web-trigger ingress (raw body, arrays, static outputs, `getUrl` — N7); lifecycle and upgrade; Realtime v2 (P15); the LLM responder (tool loop, parallel calls, failure mix, latency, list statuses, RPM/TPM); privacy mock; Rovo; module registries (N12) | 8–10 | WP0, WP1 |
| WP4 | `forge2/kit/lib/uikit-host/**`, `forge-dev uikit` | the UI Kit Node host (real reconciler and bridge, ForgeDoc snapshots, serialiser, ADS comparator, UserPicker directory, fake timers, allowlist + `unmodelled`, P16 fixtures) | 4–6 | WP0 (P16) |
| WP5 | `forge2/kit/lib/bridge-*`, `bench/forge2_probe.mjs` (browser part), `forge-dev boot report` | the browser lane: virtual frame clock, waves, CDP counters (coverage armed before the first script, initiators with the Debugger domain), `@forge/csp`, axe-core (pinned), chrome outside the frame, the widget host with two instances | 4–5 | WP1 |
| WP6 | `bench/score_forge2.py`, `bench/forge2_oracle.py`, `forge2-thresholds.json` | registry, composition, bands, criticals, ROOT_BLOCKS, the selftest, the composition Monte Carlo, the indexed oracle (per issue, per checkpoint), calibration and sha pin, controls (mutant mode, archetypes), the evidence diet (page summaries, NDJSON spill; N9), CLI refusals | 9–12 | WP2, WP3 |
| WP7 | `bench/golden-forge2/`, `bench/golden-forge2-alt/`, `forge2/mutants/` | golden (6–8, public text only, never sees the scorer); alt A (5–6, separate session, the §C.12 mechanisms); alt B (1–2, blind, another model family, cheap); ≈ 95 mutants with expectations (4–5) | 16–21 | WP8 public text, WP0 receipts, phase 4 |
| WP8 | `forge2/public/**`, `forge2/kit/bin/**` | contract, prompt, starter, `RATE-MODEL.json`, `CONTRACT-SCHEMAS.json`, `UI-HOOKS.json`; dev tools; the lint pack | 4–5 | WP0 |
| WP9 | integration.md §3–§7 | FORGE20 tier and `--forge2` flag; the per-tier `CALL_BUDGET` field; the release manifest per version; the `FORGE_BENCH` regex widened; desktop per-era tables (letters L S R T G B U K V, no E slice); site per-era snapshots and validators (the composition differs: bounded criticals, 2 bands, no excellence), refusal of an unknown era, era-scoped replace match, deploy marker, `register-forge2`, catalog proxy, the flip transaction | 5–7 | WP6 |

**Total ≈ 71–93 agent-days.** At ≈ 4 concurrent lanes the calendar is set by the critical path below, not by the sum.

### J.2 The critical path, the go/no-go, and Pilot-0

| when | what |
|---|---|
| day 0–2 | WP0 contract-gating probes start; WP1 spike; WP8 drafts the public text → **phase 3** red team → **phase 4** gate (it needs only text, so it runs in parallel with WP0 and WP1) |
| **day 3–5: WP1 go/no-go** | the pinned wrapper serving invocations in warm workers under the agent: ≥ 1,000 proxied calls/s sustained with 40–60 workers at A's scale; byte-identical double run under a CPU hog; RSS ≤ 4 GB. **GO** → the plan as written. **NO-GO (warm serving)** → fallback A: a fresh process per invocation, A at ⅓ scale, `s_tenant_isolation` graded on stored and written crossings only, and the contract says so. **NO-GO (interleaving)** → fallback B: per-key serial delivery plus SCRIPTED pairwise interleavings at the proxy (a request held until a named peer invocation parks or finishes) for `r_no_lost_update`, `s_webhook_replay` and `r_issue_exactly_once`; no global quiescence detector, no real-time settle |
| week 2 | WP2 and WP3 core; the golden backend against `forge-dev`; WP6 backend rows; WP9 desktop side (tier, kit, payload) |
| **≈ day 12–16: Pilot-0** | one Sol run from the Benchmark view (gate 3) on the then-current text, scored backend-only (owner decision, §K) |
| weeks 3–4 | WP4, WP5, golden UI, alt A, alt B, mutants; calibration passes; the gate |
| **≈ week 4–5** | the cheap-model and mid-model controls (G12) → the full Sol acceptance pilot (phase 7) |

### J.3 The freeze gate (all must hold; `score_forge2.py --reference` enforces G1–G6 and G13–G14)

| # | gate | why |
|---|---|---|
| G1 | load-robustness: golden, alt A and every racy or timing mutant scored idle and under a saturating CPU + IO hog → byte-identical verdicts apart from wall-time fields; the per-entrant replay check is wired | the property 1.0 lacked (`--quiet-load`) |
| G2 | golden 1.000 on 3 scoring + 20 calibration seeds; every critical exactly clean; no `unavailable` or `harness_missing`; every contract hook produced non-vacuous evidence | 1.0 §13.4 item 1 |
| G3 | the severity selftest and the composition Monte Carlo (§F.13) | 1.0 §8.6 + consensus finding 2 |
| G4 | alt A ≥ 0.95 with zero `harness_missing`; alt B's first build frozen, every loss classified, every contract gap fixed in text | proves the text suffices |
| G5 | each mutant loses exactly its declared rows (± ROOT_BLOCKS); racy mutants on 20/20 seeds | a race that manifests by luck is not a check |
| G6 | the archetype controls inside their bands (spread) | consensus finding 2 |
| G7 | conformance: every probe case passes its comparator class; taint tracking shows no G | §I |
| G8 | lint corpus = the real CLI (client, server, deploy legs) on every corpus manifest, including the golden's, alt A's and every mutant's | §I.3 |
| G9 | the L2 golden gate and the live platform-semantics mutants | the mutants measure Forge, not the emulator |
| G10 | the dev-world fairness invariant: every class of §D.5 at least 3 times, from the class-tagged log | §H item 4 |
| G11 | phase 4 passed and every not-derivable miss fixed in text | NOW.md phase 4 |
| G12 | one cheap-model and one mid-model entrant-path run from the Benchmark view (gate 3) before the paid pilot; every lost row of both read by an independent reader against the primary logs | the 1.0 lesson: harness defects A and B, not knowledge, decided the Haiku vs Sonnet ranking |
| G13 | scoring time measured: the golden ≤ 30 min per tree; the worst pathological mutant inside the operations bound | consensus finding 3 |
| G14 | every CTF number frozen into the contract or `RATE-MODEL.json` with golden receipts, sha-pinned (1.0's `CALIB_SHA256`); the golden's per-installation-hour ledger committed as an artefact, showing ≤ ⅔ of every budget and no wall in the hot hour on all 23 seeds | the frontier-adversary golden overspent its own cap |

### J.4 Reference app size (estimate)

| part | LOC |
|---|---:|
| intake (verify, claim, inbox, doorbell) | 250–320 |
| apply consumer (CRM, issue events, notes, idempotent create and post, per-issue pacing) | 750–950 |
| SLA engine and policy | 300–400 |
| reconcile, sweep, breach comments, leases, continuation | 500–650 |
| dosing gate (cost table, sharded accounting, pool state, header parser, pacing, retry, person budget) | 500–650 |
| access decisions, roles, admin authorization | 250–350 |
| migration (1.x reader, dual read, watermark) | 250–350 |
| JQL function + custom-field writes | 250–330 |
| LLM worker (tool loop, validation, cache, budgets, digest) + Realtime tokens | 550–700 |
| resolvers, Rovo action, export, privacy | 650–850 |
| Custom UI: board, panel, widget view and edit, shared lib, states | 1,900–2,500 |
| UI Kit admin (7 tabs) | 1,000–1,300 |
| manifest, skill, build scripts | 250–300 |
| **total** | **≈ 7,500–9,500 in ≈ 65–75 files** (≈ 4–5× Forge 1.0's golden) |

### J.5 Risks, ranked by confidence that the design holds as written (not by effort)

| # | risk | confidence | mitigation |
|---|---|---|---|
| R1 | the lockstep core with the pinned wrapper in warm workers: virtualised timers inside undici and the wrapper, determinism, throughput (machinery §4.2: MEDIUM-LOW, no precedent) | **MEDIUM-LOW** | the day 3–5 go/no-go with numbers; fallbacks A and B, each with the rows it loses named (§J.2) |
| R2 | golden convergence: 1.000 on 23 seeds with ≥ 1.5–2× margins on every bar while the naive archetypes fail them | MEDIUM | calibrate the stated numbers (never thresholds after freeze); alt A must fit the same numbers; G14's ledger artefact |
| R3 | UI Kit host fidelity (the closed product renderer) | MEDIUM | P16 fixtures; semantics only; `unmodelled` never holds a verdict |
| R4 | mock Jira breadth: security levels, role-restricted comments, moves, project deletes, comment properties, search freshness, app fields | MEDIUM | the OpenAPI-driven 501 → HOLD (1.0 R5); alt A's different endpoints prove coverage; P13/P-FIELD gate the contract |
| R5 | the custom-field and JQL semantics differ live (P-FIELD, P18) | MEDIUM | the pre-decided cut of the field rows; JQL valid through labels |
| R6 | Realtime semantics differ from the docs (1.0 defect D's history) | MEDIUM | contract text = the P15 result verbatim |
| R7 | scoring time and memory at scale | MEDIUM | the WP1 measurement before the scale numbers freeze; mutant mode; the operations bound |
| R8 | too hard: Sol < 0.30 | LOW-MEDIUM | the phase-4 gate, Pilot-0, the "too hard or harness-suspect" verdict class (§G.5) |
| R9 | too easy: Sol ≥ 0.85 | LOW-MEDIUM | the hardening levers decided in advance (§G.5) |
| R10 | ≈ 32 KB of prose invites a desk audit | LOW-MEDIUM | tables in JSON; phase 4 measures planners' reading calls |
| R11 | supply and licence: the wrapper CDN asset, OpenAPI files, `@forge/*` redistribution (1.0 R4) | MEDIUM | sha-pinned caches; install from the registry at kit time |
| R12 | calibration compute contends with the paid benchmark line on the same host | MEDIUM | G1 makes scoring load-independent, so passes run N-way in windows without a paid run; the laptop as a second scorer |

### J.6 Call budget

A disciplined frontier model is estimated at **≈ 150–230 calls**:
- reading the contract, JSON tables, typings, schema and OpenAPI: 20–30;
- writing ≈ 8k LOC: 45–70;
- lint and manifest repair: 10–15;
- dev-world cycles, browser and UI Kit checks, fixes: 70–110.

The 1.0 frontier used 50–72 of 150 calls and stopped by itself. A budget that BINDS measures speed, not depth (*"A limit
would mostly punish exploration style"*, frontier-behaviour §5.3).

**Recommended: 300 per tier** (integration D5: `CALL_BUDGET` becomes a tier field so SB7.2 keeps 150). Before freeze,
pull the 1.0 Sol and Opus call counts from the provider's generation logs to anchor this number. If the owner keeps 150,
nothing is cut from the mandate: the run becomes a speed test, and §K says so.

### J.7 Scoring time and gate compute

| step | time |
|---|---|
| lint ×2 (client + server pack), bundle (classic pragma), warm-load every function — once per tree | ≈ 1–2 min |
| per seed: the lockstep backend over 6 virtual hours + the tail (≈ 8–15k invocations, ≈ 100–200k proxied calls at ≈ 0.5–1.5 ms including scheduling) | ≈ 1.5–5 min |
| per seed: the browser lane (full on seed 1: ≈ 35 cold loads plus flows; reduced on seeds 2–3) | ≈ 3–4 min / ≈ 1–1.5 min |
| per seed: the UI Kit Node lane | ≈ 0.5 min |
| per seed: the indexed oracle and composition | ≈ 0.5 min |
| **per tree** | **≈ 15–30 min** (1.0: 6–7 min); pathological apps ≤ ≈ 60 min under the operations bound |
| mutant mode (one seed, phase-selective, backend-only unless the mutant targets a UI row) | ≈ 3–8 min |
| **one gate pass:** golden on 23 seeds (backend-heavy) ≈ 4 h + alt A ≈ 1 h + G1 double runs (golden, alt, ≈ 15 racy/timing mutants) ≈ 4.5 h + ≈ 95 mutants in mutant mode ≈ 8 h + archetypes ≈ 2 h | **≈ 20 machine-hours**, 2–3 passes; G1 makes them parallelisable 4-way (≈ 5–8 h wall per pass), in windows without a paid run |

Scoring needs no quiet machine (G1 proves it). Evidence per run is ≈ 50–150 MB: page summaries instead of bodies, NDJSON
spill, a clip ≤ 4 MiB for the site.

### J.8 Per-run model cost (estimates; the pilot replaces them with a measurement)

The anchor is Forge 1.0: Sol cost $0.72 in 13 min (≈ 2–3 M prompt tokens, ≥ 95 % cached). For 2.0, assume ≈ 3× the
calls, ≈ 2–3× the average context and ≈ 10× the output (≈ 8k LOC of code plus edits and tests).

| model | prices ($ per M: in / out / cache-read) | prompt tokens | output | per run |
|---|---|---:|---:|---:|
| GPT-6.1 Sol | 2 / 10 / 0.10 | 15–27 M (≈ 95 % cached) | 150–400 k | **$5–9, central ≈ $6.5**; arm `BENCH_MAX_USD` at $25 |
| Opus 5.5 (phase 8) | 4 / 20 / 0.20 | similar, plus ≈ 1.3× (it took 2× Sol's wall on 1.0) | similar | **$13–30**; guard $60 |
| cheap model (GPT-6 Luna class) | 0.10 / 0.50 / 0.01 | ≈ 30 M (budget exhausted) | 0.4–0.8 M | **≈ $0.5–1.0** |
| phase 8: the ≈ 31-model field rerun | — | — | — | **≈ $250–600**, dominated by the expensive models |

Scoring costs no model calls (the LLM is scripted). A broken provider cache multiplies the input side 10–20× (1.0 §11),
which is what the wallet guards are for.

### J.9 Packaging

| item | size | note |
|---|---|---|
| desktop payload (the `forge2/` trees: public, starter, kit code, site, world generator, scorer, probe, oracle, hosts, axe-core) | **+4–10 MB** | +7.1 MB more if the OpenAPI files are duplicated rather than shared with 1.0's payload |
| first-run kit download (a NEW kit lock means a new cache directory: app-modules ≈ 115 MB + the `@forge/react` 12.3.0 closure, 143.6 MB by registry metadata and ≤ 306 MB on disk + lint-modules ≈ 172 MB) | **≈ 0.45–0.75 GB** | run one real `npm ci` + `du` before freezing (BRIEF §5.0); some `@atlaskit` overlap with 1.0's kit; materialised outside the app, once per kit version |
| `@forge/bundler` (only if P25 shows a classic-pragma esbuild divergence) | +≈ 60–100 MB | measured if needed |
| SQL engine, Confluence mock, container runtime, new browser | **0** | excluded by design; Chromium/Playwright already ship |

### J.10 Integration and launch (integration.md §7, in order)

0. Forge 1.0's fate is settled: freeze with a note (owner, 2026-10-09 21:2x; workflow `forge10-freeze-note`).
1. Goose, built and gated but not released: the FORGE20 tier and flag, the 2.0 modules named so that the widened
   `FORGE_BENCH` excludes them, the release manifest per version, the scorer emitting `forge-2.0` once calibrated, the
   desktop changes (§4 of integration.md).
2. Site validators first: per-era snapshots and validation (this composition differs from 1.0's), refusal of an
   unknown era, the era-scoped replace match, per-era brief, prompt, tier meta, empty state and formula, the deploy
   marker.
3. Register forge-2.0 non-current.
4. Release the app (notarized, published, installed on the line machine).
5. The paid Sol pilot from the Benchmark view, published through the patched catalog proxy.
6. The flip, as one revision-guarded transaction.
7. Phase 8.

---------------------------------------------------------------------------------------------------------------------

## K. Owner decisions (only what genuinely needs the owner)

| # | decision | recommendation | why, and the consequence of the alternative |
|---|---|---|---|
| K1 | **Model-call budget** for Forge 2.0 (a per-tier field; SB7.2 keeps 150) | **300** | A strong model needs ≈ 150–230 calls. 250 leaves little headroom for the testing that decides the hard rows; 150 turns 2.0 into a speed test (frontier-behaviour §5.3). Weak models simply burn the budget: ≈ $0.5–1.0 each |
| K2 | **Paid-run cost for the acceptance test**: one GPT-6.1 Sol run ≈ $5–9 (central ≈ $6.5), wallet guard $25 | **approve**; a second Sol run (≈ $6.5) only if the first verdict lands in the "ambiguous" row of §G.5 | the owner defined the acceptance test as one Sol run; the guard covers twice the estimate but not a broken cache |
| K3 | **Pilot-0**: one extra early Sol run at ≈ day 12–16, scored backend-only (≈ $5–9) | **approve** | it shows where Sol loses before the UI lanes and the gate (≈ 60 machine-hours) are spent; without it the first evidence arrives after ≈ 100 % of the build |
| K4 | **Phase-8 field rerun cost**: ≈ $250–600 for the ≈ 31 models (Opus 5.5 ≈ $13–30 per run, guard $60) | **approve the envelope, run after the flip** | dominated by the expensive models; setting per-model wallet guards keeps a pathological run from eating the envelope |
| K5 | **Packaging**: a first-run kit download of ≈ 0.45–0.75 GB (new kit lock incl. the `@forge/react` 12.3.0 closure); desktop payload +4–10 MB | **accept the full closure**, measured by one `npm ci` + `du` before freeze | the alternative (the 6.2 MB runtime subset plus `d.ts`) saves ≈ 150–300 MB but removes the typings entrants discover UI Kit from, which weakens the "HOW is discoverable offline" fairness rule |
| K6 | **Machine time**: scoring 15–30 min per tree (1.0: 6–7) and ≈ 20 machine-hours per gate pass × 2–3 passes, on the workhorse (and the laptop) in windows without a paid run | **accept** | the alternative (two seeds instead of three) halves neither the gate nor the risk: worst-of-three is what stops a lucky seed from carrying a race |
