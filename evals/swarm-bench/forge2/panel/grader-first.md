# Forge 2.0 — "Escalation Desk" (panel design, angle: GRADER FIRST)

Independent design for the forge2-design-panel, 2026-10-09. Built only on `research/BRIEF.md` facts marked confirmed,
plus the measured receipts in `research/understand/*`. Nothing was run to produce it.

**The angle in one paragraph.** The toughness of Forge 2.0 should come from engineering a strong model cannot fake and
the grader can see exactly: exactly-once side effects under failures that leave the outcome unknown, lost updates under
concurrent delivery, a shared Tier 1 pool the app cannot observe below 80%, continuation across invocation limits at
real scale, a live data migration, and a world that changes while the app runs. Every one of those is graded as an
outcome (final state, call log, side-effect count) on a **lockstep virtual-time emulator**. In that emulator the
verdict is a pure function of (tree, seed), so machine load cannot move a score. Every emulated platform behaviour is
either measured on wolfaenpak by the same probe source that runs in the emulator, or quoted from a doc and stated in the
contract with every plausible reading accepted. The third class is a benchmark-world value the contract owns, used
where Atlassian publishes none. Rows resting on unstated emulator choices are refused at freeze. The UI Kit admin panel is
graded headlessly in Node against the real `@forge/react` reconciler. The browser is used only where a browser is the
measurement (Custom UI boot, CSP, XSS, theme). Out go the modules whose behaviour can be neither probed nor graded
load-robustly: Forge SQL, Confluence, workflow rules, the custom-field value function and every Preview host whose
contract nobody has measured.

| headline number | value | where |
|---|---|---|
| predicted GPT-6.1 Sol score | **0.45–0.75** (central ≈ 0.58: ≈ 0.75 if no critical fires, ≈ 0.45 if one does); reference 1.000; alt ≥ 0.95 | §G.3, §F.12 |
| spread forecast | weak 0.03–0.25 · mid 0.17–0.48 · frontier 0.45–0.75 | §F.12 |
| recommended call budget | **250** (alternative 200; 150 only with the cuts decided in advance) | §J.3 |
| public input | ≈ 27 KB prose (prompt 3 + contract 20 + starter 4) + ≈ 6 KB machine-readable tables | §E |
| per-run model cost | Sol ≈ $2.3–6.6 (central ≈ $3.8); Opus 5.5 ≈ $4.6–13; a GPT-6 Luna-class model ≈ $0.35–0.9 | §J.4 |
| scoring time | ≈ 6–10 min serial, 3 scoring seeds, no model calls; verdict identical idle vs CPU-saturated | §J.5 |
| packaging added | desktop payload +3–10 MB; kit cache +≈ 145 MB (UI Kit closure, one `npm ci`); no SQL engine, no Confluence mock | §J.6 |
| reference app | ≈ 5,900–7,900 LOC in ≈ 55–60 files (1.0 golden: ≈ 1,940 LOC) | §J.2 |
| check registry | 76 rows in 7 tiers, 5 criticals, 2 admission bands, every row continuous | §F |

---------------------------------------------------------------------------------------------------------------------

## 0. The grader this design is built around (read first: every later section assumes it)

### 0.1 Lockstep: a deterministic discrete-event emulator

Forge 1.0 graded in real time. A wait inside an invocation was wall-clock time, timeouts were real `setTimeout`s and
queues drained one event at a time, so its robustness rows were load-sensitive (`--quiet-load`). Its queues could never
interleave either (machinery N11, N1). 2.0 replaces that core with a scheduler that owns time and ordering.

1. **Warm workers.** App code runs in a pool of sandboxed Node worker processes. Each runs Atlassian's sha-pinned
   runtime wrapper, as 1.0's runner does, and loads the bundle once. Module scope survives across invocations and
   across installations, because the platform documents that warm processes MAY be reused across tenants (BRIEF §0
   fact 11). A worker serves one invocation at a time, as a Lambda does. Its only I/O channel is the platform proxy:
   the deny-default `sandbox-exec` fence is measured in 1.0 (§17 G7).
2. **A virtual-time agent in every worker.** It patches `Date`, `performance.now`, `process.hrtime`,
   `setTimeout`/`setInterval`/`setImmediate` and `timers/promises`. It reports over IPC when the invocation is blocked
   and on what (I/O, or a timer at virtual time T), so the scheduler advances time only when every live invocation is
   blocked. Two more rules keep the core deterministic.
   - A busy-wait cannot hang: every clock read while not blocked advances virtual time by a fixed 1 µs quantum. It is
     driven by the count of reads, never by CPU speed.
   - V8's `--random-seed` is set per worker from the schedule seed, so app jitter via `Math.random` is reproducible.
3. **One scheduler and one choke point.** The scheduler is a priority queue of events keyed by virtual time and
   sequence number: deliveries, I/O completions and timer wake-ups. Every proxied request completes at its request
   time plus the stated latency of its class (§E.10, `RATE-MODEL.json`). That latency carries a seeded ±20% spread,
   so different schedule seeds produce different but reproducible interleavings. One invocation runs at a time.
   Concurrency is therefore virtual interleaving at I/O boundaries, which is where production races happen too:
   Node is single-threaded per process.
4. **Kills and unawaited work in virtual time.**
   - Module timeouts (resolver 25 s; consumer and scheduled 55 s, or `timeoutSeconds` up to 900; web trigger and
     action 55 s) are enforced on virtual elapsed time. The kill is by pid (gate 4), and partial writes persist.
   - When a handler returns, its pending timers and promises are frozen. They resume only if the same worker later
     serves another invocation, and their I/O then carries the CURRENT invocation's token. That reproduces the
     documented hazard ("unawaited work may run later inside another tenant's invocation, or never", rob#12) without
     inventing any mechanism.
5. **CPU is free.** CPU time between I/O is not charged to virtual time. That is lenient and never unfair.
   - A real-time watchdog (generous, e.g. 60 s of real CPU without blocking) ends a run as `unavailable`: harness
     evidence that holds the verdict for a rescore. It is never an app zero.
6. **Hold-and-release waves for the browser lane** (BRIEF §4.3). Host-local bridge ops are answered at once.
   - Backend-bound ops (`invoke`, `requestJira`, Realtime subscribe) are queued until the frame is quiescent, then
     released as one wave.
   - The resolvers a wave triggers run in the same lockstep backend, at the checkpoint's virtual time.
   - Load stretches wall time only. It cannot change which calls the app issues before which responses.

**The property this buys:** `verdict = f(tree, fixture_seed)`. That is a freeze-gate test, not a hope (§J.1 G1): the
golden, the alt app and every mutant are scored twice, once on an idle host and once with every core saturated by a
synthetic CPU+IO hog. The verdict JSON must be byte-identical apart from wall-time fields. Forge 1.0 never had this
property.

### 0.2 Three fidelity classes, and the freeze rule

Every graded row names its basis.

| class | meaning | example | allowed to decide a row? |
|---|---|---|---|
| **M — measured** | the same probe source ran on wolfaenpak and in the emulator, with identical answers; the live answers are committed as fixtures with date, CLI, runtime and invocation ids | KVS error codes (`409 KEY_CONFLICT`, real-forge-fidelity §3.1), web-trigger request shape, the comment `properties` round trip | yes |
| **D — documented + stated** | an Atlassian doc quote no live run contradicts; the emulator's exact choice is written in the contract; the row passes every app that is correct under every plausible reading | KVS `query` eventual consistency (stated lag), scheduled-trigger duplicates, 24 h retention | yes |
| **W — benchmark world** | Atlassian publishes no value and forbids probing it, so the contract owns the number and does not claim it is production truth | the 65,000-point pool and the invisible installations, the per-endpoint cost table, bucket capacities, the CRM sender, the latency model | yes |
| **G — guess** | an emulator behaviour that is neither measured nor documented-and-stated | anything else | **no**: the freeze gate refuses any row whose evidence passes through one |

The live layer follows real-forge-fidelity §4. Real Forge decides what the offline emulator believes, and the offline
emulator decides the score. Nothing on real Forge is ever a score input, because a desktop user's machine has no
Atlassian credentials.

### 0.3 Why some modules are out (the grader-first filter)

| left out | grader-first reason |
|---|---|
| Forge SQL (GA) | SQL query limits are WALL-CLOCK (5/10/20 s per connection, 62.5 s of query time per minute, rob#60). Emulating them needs a real engine whose speed follows machine load, or an invented cost model (class G). It also adds a MySQL/TiDB engine to packaging. RFC-148 images are unverified (BRIEF §1.1) |
| Confluence (2nd product) | a second mock product. Storage-format and restriction-inheritance fidelity is MEDIUM (machinery §4.1). frontier-behaviour §5.3 judges it "mostly volume" |
| workflow validator/condition/post function | Preview; the expression forms need a Jira-expression evaluator (BRIEF §1.1) |
| `jira:customField` value function | Jira's invocation, batching and caching are unmeasured (machinery §4.7: HIGH risk); view rendering is UI Kit inside Jira's issue view |
| `apiRoute`, Object Store, app-managed permissions, `jira:command`, `global:fullPage` | Preview, and their host or request contracts are undocumented or unmeasured (BRIEF §1.1, §11.3) |
| `rovo:skill`, `rovo:mcp` | Preview, and they are graded only statically; 1.0 already covered them |
| `rovo:agentConnector`, Containers | need an external agent or a container runtime |

---------------------------------------------------------------------------------------------------------------------

## A. Product pitch

**Escalation Desk — signed customer escalations, SLA clocks and customer updates for Jira Cloud.**

B2B software companies run support in a helpdesk or CRM, and engineering in Jira. When a customer case escalates,
three things go wrong today. The CRM's webhook retries create duplicate Jira issues. Nobody can see in Jira how close
an escalation is to breaching its contractual response or resolution time. And the engineer who owns the issue spends
twenty minutes writing a customer-safe status update. Escalation Desk is the Forge app that fixes all three, at the
scale of a 6,000-issue site, on Tier 1, without starving the vendor's other customers.

- **Intake.** A signed web trigger receives escalation events from the CRM. Each CRM case becomes exactly one
  escalation, linked to exactly one Jira issue, which the app creates when the case names none. Each CRM note becomes
  exactly one Jira comment. All of this holds when the CRM retries, sends in parallel or sends out of order.
- **SLA clocks.** Response and resolution clocks are driven by the Jira issue's status history. They pause while the
  issue waits for the customer, and they follow the policy the admin sets. Clocks stay correct when issues move,
  are deleted, or change status while the app is throttled.
- **The Escalation board** (`jira:globalPage`, Custom UI). Escalation managers see open escalations by SLA state, the
  at-risk list, and live changes. It boots in one backend round trip.
- **The Escalation panel** (`jira:issuePanel`, Custom UI). Engineers see the escalation timeline and the clocks, and
  can ask Forge LLM for a customer update. The draft is built only from what the engineer may see, and it is posted
  as a Jira comment only after they confirm.
- **`issue in escalationBreached("P1")`.** A JQL function for filters, dashboards and boards.
- **The admin panel** (`jira:adminPage`, UI Kit 2). The integration secret, app roles, the SLA policy, the LLM
  budgets, failed items with retry, the app's own Tier 1 consumption, and 1.x migration progress.
- **Rovo action `get-escalation`.** Agents can answer "how close is ACME's escalation to breach?", for the asking
  person only.

**Why a customer pays for it.** It replaces a brittle CRM-to-Jira integration plus a spreadsheet of SLA dates, and an
SLA breach costs the customer money. It is RoA-eligible (static web trigger, no egress), so a regulated buyer can
install it.

**Why it is not benchmaxxed.** There is no hello-world surface, and no tutorial in Atlassian's samples does this. Every
hard requirement is one a real vendor of this app meets in production. The partner pain in the brief is the evidence:
- the Tier 1 cross-tenant denial of service, "rendering the app unusable for up to one hour on all tenants" (tiers#27);
- "50,000 issues … simply cannot complete within this limit" (t/101559);
- "under heavy load I occasionally lose increments" (CAS on CES);
- 300 customers stranded on a broken V1 (t/77751);
- "no immediate plans to support schema / breaking changes for CES" (t/96705).

The newest platform pieces (Forge LLM GA, Realtime token permissions, the JQL function contract, UI Kit 2, CES
conditional transactions) are about 35% of the surface. They are not the point: the point is a system that stays
right while everything around it misbehaves in documented ways.

---------------------------------------------------------------------------------------------------------------------

## B. Modules

Status is cited from BRIEF §1.1 (verified 2026-10-09). "Hard" says what a strong model plausibly gets wrong. The
fidelity basis of every behaviour named here is in §I.

| # | module / API | status | role in the app | why it is needed | what makes it hard |
|---|---|---|---|---|---|
| 1 | `webtrigger` `crm-intake`, `response.type: static` | GA; static required for RoA (sec#18) | CRM intake | the integration is the product | HMAC over the RAW body with a timing-safe compare; header names arrive in any case, as arrays; the replay nonce is claimed atomically (`FAIL_IF_EXISTS`, `KEY_CONFLICT`) with a TTL that covers the window, and an expired-but-readable nonce must be read through its `expireTime` (rob#47); no side effect before verification; acknowledge fast and enqueue; static `outputs` only |
| 2 | Async events: `consumer` ×2 (`work`, `llm-work`), `@forge/events` 3.0.7 | GA (v2+ retry semantics, rob#27) | all background work | the 55 s web trigger, 25 s resolver and 900 s consumer limits force work off the request path | at-least-once and unordered delivery; `jobId` is not an idempotency key; `InvocationError` with `retryAfter` ≤ 900 s versus a quota wall of up to ~45 min; concurrency keys are per installation across queues; 50 events / 200 KB per push; 100 KB per event for long consumers; the cyclic limit of 1,000; v1-shaped events still queued from 1.x |
| 3 | `trigger` on `avi:jira:updated:issue`, `avi:jira:deleted:issue` (plus the lifecycle `avi:forge:upgraded:app`) | GA | SLA clock inputs, moves and deletes | the clocks are driven by Jira | delivery up to 3 min late; the app's own writes come back as `selfGenerated` unless `ignoreSelf` (rob#35); only top-level deletes are emitted (rob#36); 403s during the install race (rob#37) |
| 4 | `scheduledTrigger` ×2 (`hour` reconcile, `fiveMinute` SLA sweep) | GA; ≤ 5 per app, ≤ 1 `fiveMinute` (tiers#39) | backfill, heal, breach sweep, JQL precomputation refresh | the event stream is not trusted (BRIEF §8) | duplicates are possible, a throw is not retried, runs can overlap or be skipped; spread over the hour; resumable chunks through the queue |
| 5 | `jira:jqlFunction` `escalationBreached` | GA; contract captured 2026-10-09 (BRIEF §1.1, §11.3) | filters and dashboards | customers live in JQL | precomputations are NOT per user (a caller-dependent result leaks or lies); results must be refreshed through the computation API when an SLA state changes; ≤ 1,000 right-hand values (the large site exceeds it); 25 s limit |
| 6 | `jira:globalPage` `board` (Custom UI) | GA; ONE per app ("deployment will fail" otherwise) | the Escalation board | mandate: a pretty front-facing Custom UI | boot budget in waves, ops and bytes; Realtime without polling; correct while the pool is walled; per-person visibility with a verdict TTL; XSS from CRM text |
| 7 | `jira:issuePanel` `panel` (Custom UI) | GA | timeline, clocks, the LLM draft | the engineer's daily surface | `extension.issue.id` is not proof of permission (sec#1); confirm-before-post; exactly one comment per confirmed draft; boot budget |
| 8 | `jira:adminPage` `admin`, `render: native` (UI Kit 2, `@forge/react` 12.3.0, React 18.3.1) | GA (uikit#18,20); subpages are Custom UI only, so navigation is Tabs | secrets, roles, policy, budgets, failures, usage, migration | mandate: a UI Kit admin panel | Form `onSubmit` arrives with no data (`handleSubmit` is the gate); UserPicker stores an object, so the app maps `.id`; DynamicTable sorts by cell `key` with ADS numeric collation (decimals and display dates mis-sort); `testId`s only where delivered; every admin resolver authorised server-side |
| 9 | `llm` (`@forge/llm` 1.0.7) | GA 2026-07-29/30 | customer-update drafts; the weekly digest | mandate: good Forge LLM usage | no structured output, so a forced tool is the only contract and arguments arrive unvalidated; `list()` statuses; sampling-parameter rules; streams that end without `finish_reason`; 100 RPM per installation and 500k TPM per model; credits billed to the developer; long jobs belong in a consumer of ≥ 300 s; prompt injection; per-user scope of every prompt |
| 10 | Forge Realtime (`@forge/realtime` 1.0.1, bridge `realtime.*`) | GA; publish-only and subscribe-only tokens since CHANGE-3326; 50 ops/s per installation | live board, draft delivery | the documented alternative to polling (rob#73) | claims must be derived server-side (Atlassian's own tutorial is exploitable, llm "newer facts" 5); publish/subscribe pairing; coalescing under 50 ops/s; `publish` returns `errors[]` and does not throw |
| 11 | KVS + Custom Entity Store (`@forge/kvs` 2.0.7) | GA | all state | — | conditional CES transactions are the only atomic multi-key tool (≤ 25 operations, no `keyPolicy` inside); TTL is read from the 4th argument of `transact().set` (rob#50); `query` is eventually consistent; cursors must not be persisted; 10 KB write units and a hot per-key 1 MB/s limit hit any single counter key; the server rule allows exactly one range attribute per index |
| 12 | `kvs.setSecret` / `getSecret` | GA | the CRM shared secret | sec#22 | presence-only read-back; never in logs, responses, Realtime or prompts |
| 13 | `action` `get-escalation` + `rovo:agent` | GA (action page has no Preview label; `rovo:agent` carries no Preview marker, page not re-fetched — BRIEF §1.1) | agents; the auditor read path (§C.3) | Rovo is where customers ask | action inputs never authorise (sec#26); identity comes from context; permissions are re-checked |
| 14 | bridge `requestJira` (Custom UI) with forwarded rate-limit headers | GA (FRGE-1923 fixed 2026-04-09) | person-scoped reads | the stated points exemption is a real dosing lever (tiers#24,25) | runs as the user (no asApp), still subject to burst buckets, and must never be trusted by a resolver as proof |
| 15 | `invoke(..., { rateLimitProperties: true })` | GA (CHANGE-3314) | boot under a 429 | B8 | metadata field names: docs `rateLimitValue` vs typings `rateLimitLimit` (the emulator returns both, stated) |
| — | Custom UI multi-entry resources (`entry`) | GA (CHANGE-3337) | ALLOWED, never required | bundle sharing between the board and the panel | demanded only after probe P-ENTRY proves the bridge is injected into named entries on wolfaenpak (BRIEF §1.1 risk) |

**Scopes** come from the shipped OpenAPI per call, as in 1.0's `l_scopes`. **Runtime** is `nodejs22.x` or
`nodejs24.x`: `nodejs20.x` is a hard server-side refusal (real-forge-fidelity §2.3 v08).

---------------------------------------------------------------------------------------------------------------------

## C. Architecture (the reference design; the contract prescribes outcomes, never this shape)

### C.1 Data model

The contract fixes no v2 storage shape: any layout passes, because everything is graded through behaviour (§F). It
fixes the 1.x layout, which is an input (§C.9), and the export schema (§C.3).

Reference layout. Every CES index has exactly one `range` attribute, because the server rule refuses more (Forge 1.0
§17.8 E); there are at most 20 entities and 7 indexes per entity.

| store | key | content | index (partition → range) | why this shape |
|---|---|---|---|---|
| CES `escalation` | `esc:<caseId>` | caseId, issueId, projectId, priority, crmState, seq (last applied CRM sequence), policyVersion, openedAt, activeMs, pausedSince, respondedAt, resolvedAt, responseDueAt, resolutionDueAt, breachState, issueState (`live`/`moved`/`deleted`), customer, version (integer, optimistic concurrency), updatedAt | `by-project` (projectId → resolutionDueAt), `by-issue` (issueId → updatedAt), `by-breach` (breachState → resolutionDueAt) | every writer is a conditional transaction on `version`, so concurrent writers retry instead of losing updates |
| CES `note` | `note:<caseId>:<noteSeq>` | text hash, status (`pending`/`posting`/`posted`/`failed`), commentId, attempts, marker | `by-status` (status → updatedAt) | the posting state machine behind exactly-once comments |
| CES `failure` | `fail:<kind>:<id>` | reason, attempts, firstSeenAt, lastError | `by-kind` (kind → firstSeenAt) | the admin's failed list |
| KVS | `dlv:<sha256(deliveryId)>` | `{at}`; `keyPolicy: FAIL_IF_EXISTS`; TTL 24 h; reads check `expireTime` | — | atomic replay nonce. Raw ids are hashed because of the key regex (sec §3) |
| KVS | `payload:<deliveryId>` | the raw CRM body; TTL 24 h | — | events carry ids, not payloads (100 KB per event for long consumers, rob#16) |
| KVS | `role:<accountId>` | `{role, grantedBy, at}` | — | app roles; the platform has none (sec#9) |
| KVS | `verdict:<accountId>:<issueId>` | `{browse, at}`; TTL 10 min; reads check `expireTime` | — | the stated permission-verdict TTL (§E.3) |
| KVS | `pts:<installHour>:<shard>` | points spent, sharded 8 ways, flushed once per invocation | — | per-installation self-accounting. A single hot key is limited to 1 MB/s and burns a 10 KB write unit per increment (rob#52,53) |
| KVS | `pool:state` | `{pausedUntil, reason, nearLimitUntil}` | — | the pool pause shared by every invocation of this installation |
| KVS | `llm:<sha256(install, scopeHash, contentHash, model)>` | the draft; TTL 7 d | — | the stated LLM cache key (§E.7) |
| KVS | `mig:watermark`, `mig:stats` | the last migrated 1.x key, counts | — | resumable migration. Cursors are not persisted (rob#57) |
| KVS | `policy:current`, `policy:<v>` | SLA minutes per priority, pause statuses, version | — | policy changes apply from the moment they are saved |
| KVS | `jqlfn:state` | the breach set version and the last precomputation sync | — | keeps the JQL function fresh |
| secret store | `crm-secret` | the shared HMAC secret (`setSecret`) | — | presence-only read-back |

**Transactions.**
- **Escalation apply.** A CES `transact()` sets `escalation` with the condition `version == v`, writes up to 23 `note`
  rows, and checks `version`. On `CONDITIONAL_CHECK_FAILED` the writer re-reads with `get`, which is strictly
  consistent, and retries at most 5 times. After that it re-enqueues with a delay, never spins.
- **No `keyPolicy` inside batches or transactions** (rob#46). The nonce claim is a single `kvs.set` with
  `FAIL_IF_EXISTS`.
- **Measured conflict codes.** Real Forge answers `409 KEY_CONFLICT` for `FAIL_IF_EXISTS` and `400
  CONDITIONAL_CHECK_FAILED` for a failed transaction condition. The emulator must answer exactly that (probe P01).

### C.2 Async topology

```
CRM ──signed POST──▶ webtrigger crm-intake (55 s; sender waits 10 virtual s)
                      verify HMAC + window → claim nonce (FAIL_IF_EXISTS) → store payload (TTL) → push {kind:'crm', deliveryId}
                      → outputKey 'accepted' (202)  |  'unauthorized' (401)  |  'malformed' (400)
Jira ──avi:jira:updated:issue / deleted:issue (filter ignoreSelf)──▶ trigger issue-events
                      → push {kind:'issue', issueId, changelogIds}   (no Jira call in the trigger)
queue `work`  ──▶ consumer apply (timeoutSeconds 120, concurrency {key:'esc-'+bucket(caseId), limit 1})
                      crm:   load payload → resolve/create issue (idempotent, §C.8) → conditional transact escalation
                             → notes → post comments (idempotent, paced per issue) → publishGlobal(board-channel, {caseIds})
                      issue: read status history (changelog bulkfetch, fields filtered) → recompute clocks → transact
                      v1:    1.x-shaped bodies {caseId, payload} → same path (old events reach new code, rob#32)
                      chunk: continuation of a backfill / migration / digest slice (watermark in KVS, cursor never stored)
scheduled hour ──▶ reconcile: jittered start inside the hour → incremental JQL since watermark − margin
                      → push chunk events of ≤ 50 per push, ≤ 200 KB
scheduled fiveMinute ──▶ sweep: lease (FAIL_IF_EXISTS + TTL + expireTime) → by-breach range query ≤ now
                      → breach transitions → JQL precomputation update → publish
queue `llm-work` ──▶ consumer draft (timeoutSeconds 300) → chat() with forced tool → validate → cache
                      → publishGlobal(per-user token channel)
lifecycle avi:forge:upgraded:app ──▶ push {kind:'migrate', from:'1.x'} (the migration also starts lazily on first read)
```

**Continuation across invocation limits.**
- Every long loop checks `getAppContext().invocationRemainingTimeInMillis()`. Before the budget runs out it commits a
  watermark and pushes its own continuation, batching up to 50 events per push so the 1,000-push cyclic limit holds
  (rob#18).
- A Jira `Retry-After` or a pool reset longer than the remaining time becomes `return new InvocationError({retryAfter:
  min(wait, 900), retryReason: FUNCTION_UPSTREAM_RATE_LIMITED})`. If the wall is longer than 900 s, the work is
  re-enqueued with `delayInSeconds` and checked again on delivery. Nothing busy-waits.
- A poison item (unknown issue key, deleted issue, schema-invalid payload) moves to `failure` with its reason after
  ≤ 5 attempts. Everything else continues.

**Install race.** The first asApp calls of a fresh installation may answer 401/403 (rob#37). They are retried through
`InvocationError` with `retryAfter` ≥ 60 s, and never treated as a permanent failure inside the stated window (§E.10).

### C.3 Resolver catalogue

Every resolver returns a value and never throws (1.0 contract rule, kept). Refusals are `{ "error": "forbidden" }`.
Identity is ALWAYS `context.accountId`; payload identity, roles and flags are ignored (sec#1,2). `context.extension.*`
is treated as untrusted: the contract says the harness does not validate it (§E.3).

| key | surface | inputs | authorisation (server-side, every call) | Jira calls (principal) | failure handling |
|---|---|---|---|---|---|
| `board.bootstrap` | globalPage | `{filter?}` | licensed user; escalation rows only for issues the caller can browse, using a verdict ≤ 10 min old (else a fresh check) | asUser `search/jql` with `id in (candidates)&fields=id` for verdicts — one call per ≤ 100 candidates (1 + visible) | pool paused → `{paused:{until}, rows: only rows with fresh verdicts}`; never throws |
| `board.page` | globalPage | `{after: <dueAt,caseId>, filter}` | as above | as above | key-range watermark, never a stored cursor |
| `board.subscribeToken` | globalPage | `{}` | licensed | none | `signRealtimeToken` with SUBSCRIBE-only permissions and claims `{accountId}` derived from context |
| `panel.bootstrap` | issuePanel | `{}` (issue id from `extension.issue.id`) | the caller must browse THAT issue (live verdict or a cached one for this user and issue) | asUser verdict if not cached | as above |
| `panel.requestDraft` | issuePanel | `{caseId}` | browse on the escalation's issue; the per-user throttle (§E.4); the user and installation credit budgets (§E.7) | none (the consumer reads) | enqueues to `llm-work` with `requester = context.accountId` (server-derived); returns `{jobId}` or `{error:'budget'\|'throttled'\|'forbidden'}` |
| `panel.postDraft` | issuePanel | `{draftId, confirm:true}` | the draft must belong to `context.accountId` and the escalation (IDOR); the caller must be allowed to comment | asUser `POST /issue/{id}/comment` (ADF) carrying `properties:[{key:'escalation-desk', value:{draftId}}]` | the idempotency marker (§C.8) makes double confirms and ambiguous failures land exactly one comment |
| `admin.getConfig` | adminPage | `{}` | desk-admin (§E.3) | none | the secret as `{present:true\|false}` only; the webhook URL via `webTrigger.getUrl` |
| `admin.setSecret` | adminPage | `{secret}` | desk-admin | none | `setSecret`; never echoed or logged |
| `admin.grantRole` / `admin.revokeRole` | adminPage | `{accountId, role}` | desk-admin; no self-grant; the last desk-admin cannot be removed | asApp user lookup (2 points) — cached per installation | refusal shapes stated |
| `admin.setPolicy` | adminPage | `{priorities:{P1..P4:{responseMin, resolutionMin}}, pauseStatuses[]}` | desk-admin | none | validates integers in stated ranges; increments the version; takes effect from the save instant |
| `admin.setBudgets` | adminPage | `{userCreditsPerDay, installCreditsPerDay}` | desk-admin | none | — |
| `admin.failures` / `retryFailure` / `dismissFailure` | adminPage | `{page}` / `{id}` | desk-admin | none | — |
| `admin.usage` | adminPage | `{}` | desk-admin | none | `{pointsThisHour, pointsByHour[], refusals:{reason:count}, pausedUntil, llmCreditsToday, projectedUsd}` from self-accounting |
| `admin.migration` | adminPage | `{}` | desk-admin | none | `{v1Total, migrated, remaining, done}` |
| `admin.startDigest` | adminPage | `{}` | desk-admin | none | enqueues digest slices |
| `admin.export` | adminPage | `{caseIds[] ≤ 100}` | desk-admin | none | the canonical escalation JSON (`CONTRACT-SCHEMAS.json#/escalation`): **the auditor read path the grader uses**, a natural compliance-export feature |
| `get-escalation` | Rovo action (GET) | `{caseId}` | the invoking person must browse the issue; an `accountId` in the inputs is ignored | asUser verdict | `{error}` for unknown, missing or invisible, never throws |
| `escalationBreached` | JQL function | `(priority?)` | none: the result must not depend on the caller (precomputations are shared across users) | none | returns a JQL fragment; above 1,000 matches, a fragment that is not an id list (§C.10) |

Desk-admin bootstrap: holders of Jira global `ADMINISTER` are implicit desk-admins, checked live with
`permissions/check` `globalPermissions:["ADMINISTER"]` or `mypermissions?permissions=ADMINISTER` as the user, with a
verdict TTL of ≤ 5 virtual minutes (sec §2). `authorize()` has no ADMINISTER helper and throws outside user-invoked
modules (sec#12).

### C.4 The Tier 1 dosing the app implements (stated rules in §E.4; numbers in `RATE-MODEL.json`)

1. **Self-accounting.** Every product call's points are computed from the stated cost table, using the response
   (objects returned). They are added to an in-invocation tally and flushed once per invocation to the sharded hourly
   counter. Interactive calls (resolver or action invoked by a person) and background calls are kept apart.
2. **Per-installation background budget.** At most **12,000 points** per virtual UTC hour (stated, W), and no 5-minute
   window may hold more than **25%** of that hour's background spend (stated smoothness). Scheduled work starts at a
   seeded jitter inside the hour and paces itself.
3. **Observing the pool.** The app parses `RateLimit`/`RateLimit-Policy` as comma-separated quoted policies in any
   order, with `r` optional and `Beta-` informational (tiers#18,19).
   - When `"global-app-quota"` shows `r < 0.20·q`, background work pauses until `t` elapses (written to `pool:state`).
     Interactive work continues.
4. **The wall.** On any quota-class 429, every invocation of the installation makes no product call until the reset,
   reading `pool:state` before each call. Quota-class means `jira-quota-*`, an unknown reason, or the gateway variant
   with no `Retry-After`, where `X-RateLimit-Reset` is used. Deferred work re-enqueues itself in ≤ 900 s hops, and
   views show the paused state (§C.6).
5. **Burst.** `jira-burst-based` slows only THAT endpoint and method, for `Retry-After` seconds. The app paces each
   endpoint below the stated steady-state rate.
6. **Per-issue writes.** Writes to one issue are paced at ≤ 20 per 2 s and ≤ 100 per 30 s. The hot case (many notes
   at once) is queued per issue, never fired in parallel.
7. **In flight.** At most 4 product requests in flight per invocation (stated; Atlassian's 5–10 is advice, tiers#22).
   Concurrency across invocations is bounded through concurrency-key limits.
8. **Retries.** Never before `Retry-After` (or the reset). At most 4 attempts per request, then defer through the
   queue. Never a blind retry of a non-idempotent write (§C.8).
9. **The bridge lever.** Person-scoped issue fields on the board and panel (summary, status, assignee) come from bridge
   `requestJira` in the browser. That is exempt from points and from the pool wall by stated rule (tiers#24,25), but
   subject to burst. Escalation data and verdicts still go through resolvers.
10. **The button-masher.** Expensive person actions (draft requests) are limited per user (§E.4).

Why this is genuinely hard (frontier-behaviour §5.2 items 1–2): the app cannot see the pool below 80%, has no
cross-installation storage, must keep hot counters off a single key, and must finish a 6,000-issue backfill plus a
1,500-delivery burst inside its budget AND inside the freshness objectives.

### C.5 Forge LLM features

1. **Customer update draft** (issue panel).
   - The prompt is built in the `llm-work` consumer from the escalation record plus the issue's comments and status
     history.
   - The trap is that a person who can browse the issue may still be barred from some of its comments, because Jira
     comments can be restricted to a project role or group; installation B seeds such comments.
   - The reference reads the comments asApp, then keeps only those whose `visibility` the requester satisfies, using
     the requester's role memberships, read asApp and cached under the verdict TTL.
   - Offline user impersonation exists. The limits page lists "Offline user impersonation tokens, 1,000 requests per
     minute, per app", but its manifest switch is not in the verified brief. It is therefore verified in phase 3
     before the emulator models it.
   - The grader accepts any mechanism that puts exactly what the requester may see into the prompt.
   - It calls `chat()` with `tools: [customer_update]` and `tool_choice: {type:'function', function:{name:
     'customer_update'}}`. The model is the first `active` Sonnet-tier id from `list()` at call time, with
     `max_completion_tokens` set and no `temperature`/`top_p` pair (llm#16,17).
   - The tool arguments are validated against the stated JSON schema; a JSON-string `arguments` is parsed, not
     rejected.
   - Cited issue keys are kept only if the requester can see them. The summary is rendered as text: no HTML, no
     links. The draft carries the stated AI label.
   - The result goes to a per-user channel over Realtime. The draft is cached by
     `(installation, requester-visibility scope hash, content hash, model)`.
   - Posting requires the explicit confirm resolver (§C.3). A truncated stream (no `finish_reason`) is resumed once
     from the prior context, then reported as failed (llm#11,12).
2. **Weekly digest** (admin). One call per open P1/P2 escalation, built ONLY from CRM-sourced fields: customer,
   priority, clocks, CRM state. That avoids per-person visibility, because the audience is desk-admins. It uses the
   cheapest active tier (an id containing `haiku`, stated) and is paced at ≤ 100 requests per 60 s and ≤ 500k tokens
   per 60 s per model, by the stated token estimator. An unchanged rerun costs 0 calls (cache).
3. **Budgets.** Per-user and per-installation credits per virtual day, set in the admin panel. Credits are priced by
   tier (Haiku 10 / Sonnet 30 / Opus 50 per 1M tokens, $0.10 in / $0.50 out per credit; labelled an assumption for
   unpublished models, llm#32). After exhaustion: 0 calls and the stated message.
4. **Errors.** `ForgeLlmAPIError` is not exported, so the app detects it by `err.name` or a numeric `err.status`
   (llm#36). A refusal (no tool call), malformed arguments, 403 `FORGE_LLMS_MODEL_FORBIDDEN`, 429 and 5xx each map to
   the stated UI state with at most 1 retry, no storm, and the non-AI features keep working.

### C.6 Front-facing Custom UI surfaces and the boot budget

| surface | SHELL (stated selector) | READY (stated selector, verified against seeded CONTENT) | live |
|---|---|---|---|
| board (`jira:globalPage`) | `[data-testid="board-shell"]`: header, four KPI tiles (`on-track`, `at-risk`, `breached`, `paused`), an empty table | `[data-testid="board-ready"]` with the viewer's first page of at-risk rows (`tr[data-case-id]`) equal to the oracle at the checkpoint | Realtime: after a recorded change the board shows it within one release wave of the publish; zero invokes while idle |
| panel (`jira:issuePanel`) | `[data-testid="panel-shell"]` | `[data-testid="panel-ready"]` with the escalation timeline (`li[data-seq]`) and the clocks (`time[data-clock="response\|resolution"][datetime]`) | the draft arrives over Realtime (`[data-testid="draft"]`) |

**Boot design.**
- Render the shell synchronously at module load; `await view.theme.enable()` is free and allowed.
- Issue ONE bootstrap `invoke` and ONE bridge `requestJira` (`search/jql` with `fields=summary,status,assignee` for the
  page's issue ids, once the ids arrive; or in parallel for the panel, whose issue id is in the context) in the first
  wave.
- Subscribe to Realtime after READY. No flags SDK. A production React build, one entry per surface, about 60–120 KB
  gzip including React.
- On a 429 from the bootstrap invoke (`rateLimitProperties`), keep the shell and retry exactly once at or after
  `rateLimitReset`.

**Pool wall.** The views show `[data-testid="quota-paused"]` with `<time datetime>` equal to the reset instant, and
only the rows the TTL rule allows.

**Design system.** `var(--ds-*)` tokens, light and dark, contrast ≥ 4.5:1, painted surfaces, no horizontal scroll at
1280 and 380 px. Beauty itself is NOT scored; screenshots are published for people to judge.

### C.7 The UI Kit admin panel (`jira:adminPage`, `render: native`)

`Tabs` (`testId="admin-tabs"`; uncontrolled is allowed) with seven panels. Each graded control has a stated `testId`
on a component that delivers one (uikit#31).

| tab | components | action → stated resolver call |
|---|---|---|
| Integration | Heading, Text (webhook URL), Lozenge `secret-state` (`Set`/`Missing`), Form with Textfield `type="password"` | Save → exactly one `admin.setSecret {secret}`; empty → ErrorMessage, zero invokes |
| Roles | DynamicTable `roles-table`, UserPicker, Select role, Button `grant`, Modal `revoke-confirm` | grant → one `admin.grantRole {accountId: <string>, role}` (map `onChange(...).id`, uikit#29); revoke via the Modal confirm only |
| SLA policy | Form: Textfield per priority/clock (`type="number"`), Checkbox pause statuses | invalid (non-integer, out of 1–10,080) → ErrorMessage per field, zero invokes; valid → one `admin.setPolicy` with integers; double activation → one invoke |
| Budgets | Textfields, Button `start-digest` | one `admin.setBudgets` / `admin.startDigest` |
| Failures | DynamicTable `failures-table`, columns `attempts` (integer key), `first-seen` (ISO key), `age-hours` (decimal key), Button per row `retry` | header clicks → the stated order; numeric keys required; `1.5` vs `1.05` and display dates mis-sort with string keys (uikit#11) |
| Usage | Text `usage-points`, `usage-paused`, `usage-credits`, DynamicTable `usage-refusals` | display of `admin.usage` |
| Migration | ProgressBar, Text `migration-remaining` | display of `admin.migration` |

**Server-side authorisation** of every admin resolver (S1). Hiding the page or tab is never authorisation; the admin
page's own reachability is not a control (sec §2, ECO-1592).

**Boot.** ≤ 1 invoke before the first ForgeDoc that holds the default tab's content, and the first ForgeDoc is
non-empty (a loading state).

### C.8 Web triggers and integrations

**The CRM sender is the benchmark's own world (W), stated in full in §E.2.**
- Headers: `x-desk-delivery` (uuid), `x-desk-timestamp` (unix seconds), `x-desk-signature: v1=<hex HMAC-SHA256(secret,
  timestamp + "." + raw body bytes)>`.
- Accepted within ±300 s of the harness clock.
- Retries with the same delivery id on non-2xx or no answer within 10 virtual s, up to 6 attempts over 30 virtual
  minutes, and may run up to 4 deliveries concurrently.
- The platform `request.authentication: hmacSharedSecret` is neither required nor penalised (BRIEF §0 fact 9).

**Exactly-once writes to Jira under ambiguous failure** (frontier-behaviour §5.2 item 3). Both guards rely on the
documented fact that a write may be applied while the response is lost.
- **Comments.** Every comment the app posts carries the entity property `escalation-desk` = `{noteKey}` (the shipped
  OpenAPI's `Comment` bean has `properties`). Before re-posting after a lost response or a 5xx, the app lists the
  issue's comment ids with `GET /issue/{id}/comment`, which documents only `expand=renderedBody`. It then reads their
  properties with `POST /rest/api/3/comment/list?expand=properties`, which documents the `properties` expand, and posts
  only if no comment carries that marker. A marker in the comment body text is an equally valid guard, and the grader
  accepts either.
- **Issues.** Creation carries an issue property and a label (`esc-<caseId>`). After an ambiguous failure, the app
  searches `labels = esc-<caseId>` (or the property) only after the stated search-freshness delay, or passes the
  candidate ids in `reconcileIssues` when it has them. It never creates twice.

### C.9 Brownfield: Escalation Desk 1.x data on the scoring installations

1.x data is present on installations A and B at install time (stated layout, §E.9):
- KVS `esc1:<caseId>` → `{caseId, issueKey, priority, status, seq, customer, dueAt}`. `issueKey` is a KEY, not an id,
  and some issues have since moved projects. `dueAt` is an ISO string.
- KVS `esc1idx:<priority>:<caseId>` index keys.
- 1.x-shaped events `{caseId, payload}` still pending on queue `crm-events`.

v2 must do four things.
- Consume `crm-events` (old events reach new code, rob#32).
- Migrate every 1.x record exactly once, resolving keys to ids. GET by an old key returns the moved issue, probe P13.
- Serve every escalation correctly at every checkpoint (dual read: v2 first, else 1.x with lazy migration).
- Delete `esc1:`/`esc1idx:` keys only after migrating them. The migration is resumable through the watermark and
  chunk events; nothing is lost if a chunk is killed at its timeout.

### C.10 The JQL function at scale

`escalationBreached(priority?)` returns `id in (…)` while ≤ 1,000 issues match. Installation A has more, so beyond that
the reference returns `labels = esc-breached` (maintained by the sweep as a paced write per issue). Any fragment Jira
accepts and the mock models passes; the mock models labels, ids and, if probe P18b confirms the module, indexed entity
properties. Precomputations are refreshed through `GET/POST /rest/api/3/jql/function/computation` whenever the breach
set changes (BRIEF §1.1). The result never depends on the caller, because precomputations are shared across users and
Jira applies the searcher's permissions afterwards.

---------------------------------------------------------------------------------------------------------------------

## D. The world

### D.1 Scoring sites (per scoring seed; three seeds per tree, worst per row)

One generator, pure and seeded (the 1.0 discipline, with machinery N3 fixed: a numbered vocabulary, re-banded id
classes, indexed simulation). The ranges are generator policy. The scorer reads the actual pack, never these numbers.

| installation | role in the scenario | scale (per seed) |
|---|---|---|
| **A "large"** | throughput, budget, migration, JQL scale | 4 projects, 5,000–7,000 issues, 2,000–2,800 escalations (60% open), 1.x data on 1,500–2,000 of them, 35–50 users, CRM traffic including one burst of 1,200–1,600 deliveries within 10 virtual minutes, more than 1,000 breached issues at the peak |
| **B "permission-rich"** | visibility, world changes, ambiguous writes | 2–3 projects, 1,200–1,800 issues, 300–500 escalations, issue security levels, a reporter-only browse project, role-restricted comments on browsable issues, project-role changes, moves and deletes, 1.x data on about 200 |
| **C "small interactive"** | interactive reserve, boot, LLM | 1 project, 150–250 issues, 40–80 escalations, persons loading the board and panel and drafting throughout |
| **invisible installations** | the shared pool | not addressable. The gateway charges a seeded per-hour spend profile to the same 65,000-point pool, so the pool runs hot in at least one hour and walls in at least one other |

Scale is announced in the contract as upper bounds only ("up to 8,000 issues and 3,000 escalations per installation;
CRM bursts of up to 2,000 deliveries in 10 minutes"). The exact numbers are seeded.

### D.2 The virtual timeline (six virtual hours; the scorer's private plan, not in the contract)

| virtual time | happens on the scoring sites | graded at |
|---|---|---|
| H0+0 | 2.0 replaces 1.x; `avi:forge:upgraded:app`; 1.x events pending; first asApp calls 403 for a seeded window | — |
| H0+5m … | scheduled ticks begin ("about 5 minutes after deployment", rob §8); CRM trickle on every installation | — |
| H1+0…10m | A's CRM burst: duplicates, concurrent same-case deliveries, out-of-order `seq`, a hot case with many notes | CP1 (H1+30m): export, Node UI Kit lane |
| H1–H2 | ambiguous Jira write outcomes; issue moves, deletes, a project delete; status churn | — |
| H2+0…20m | B: role and permission revocations; an admin policy edit through the admin resolver as a desk-admin | CP2 (H2+40m): export, browser lane |
| H3 | invisible spend drives the pool past 80% → `r` appears | — |
| H4 | invisible spend walls the pool for ~25–45 min | CP3 (inside the wall): browser lane |
| H4–H5 | LLM drafts by C's persons; the admin digest on A; the button-masher on C | — |
| H5 | quiet: everything must converge; duplicate, overlapping and skipped scheduled ticks across the run | CP4 (H5+50m): final export, browser lane, Node lane, Jira-side counts |

Between checkpoints, background time runs in lockstep. At a checkpoint the background clock freezes while the UI
lanes run, so surface loads are served at the checkpoint's virtual time.

### D.3 Concurrency the world produces (stated world values, W)

- The harness runs at most 8 async-event deliveries of one installation at once (fewer when the app's concurrency
  keys say so). Product-event triggers run up to 2 at once; the CRM sender sends up to 4 deliveries at once; a
  person-facing surface may have 2 resolver calls in flight.
- Workers are reused across installations in seeded order (warm reuse), and are sometimes replaced (cold).

### D.4 Faults and mid-run changes, expressed as GUARANTEES

The contract never lists what the scorer injects, when, where or how often. It states:
- the guarantees the app keeps (§E.1–E.9);
- the documented platform facts the harness reproduces (§E.10 — Atlassian's documented behaviour, each with its doc
  URL);
- the benchmark-world facts it owns (the CRM sender, the pool and its other installations, the cost table, the
  latency model).

Every behaviour the scorer exercises belongs to one of those three, so a correct app needs no knowledge of the plan.

| the scorer exercises (private) | the class the contract states (public) | the guarantee that grades it |
|---|---|---|
| duplicate, delayed, reordered and concurrent async deliveries; redelivery after a kill that followed a side effect | Forge: async events are delivered at least once, in no guaranteed order, and retried within 24 h | G1, G2, G3 |
| a Jira write applied but answered 5xx, or its response lost; search not yet showing a new issue | world: "a request may fail after the server applied it"; Jira: `search/jql` may not show recent writes unless `reconcileIssues` names them | G2, G3 |
| a pool wall of up to ~45 min; `r` visible; gateway 429 without `Retry-After`; burst and per-issue 429s | world: pool, invisible installations, header grammar, buckets (`RATE-MODEL.json`) | G7 |
| issue moved, deleted, project deleted; role revoked; policy edited | Jira: issues move and change key, are deleted (only top-level delete events), permissions change; policy changes come from the admin | G4, G5, G6 |
| duplicate, overlapping and skipped scheduled ticks | Forge: duplicates possible, no retry after a throw | G1–G4 |
| kills at module timeouts; unawaited work resumed in another tenant's invocation | Forge: timeouts per module; warm reuse across tenants; unawaited work may run later or never | G1, G9 |
| LLM refusals, malformed tool arguments, 403/429/5xx, truncated streams, prompt injection in issue text | Forge LLM: documented failure modes; model output is untrusted (stated rule) | G8 |
| replayed, forged, stale and case-varied web-trigger requests | world: the CRM spec; the URL is public | G6 |

### D.5 Dev site versus scoring sites

| | dev site (what the entrant's `forge-dev` serves) | scoring sites |
|---|---|---|
| seed | `dev_seed`; the scorer refuses `--seed == dev_seed` (1.0) | the run's `fixture_seed` + 2 derived |
| installations | 2 (an A-like at ~1,500 issues with > 1,000 breached for the JQL cap, and a C-like) plus invisible spend | 3 + invisible spend |
| fault plan | its own seeded plan. **Fairness invariant: every behaviour class of §D.4 occurs at least once during `forge-dev world run --hours 6`.** WP1 proves it from the dev world's event log, which carries a class tag per event | different timing, targets and volume |
| messages | **exactly what real Forge or Jira print**: measured codes and messages (P01 etc.). No harness hints such as "you forgot X" | same |
| tools | `forge-dev world run [--hours N] [--until <event>]` (lockstep, fast-forward); `forge-dev ledger` (per-call points, the dev "test quota" layer Atlassian staff recommend, tiers §5); `invoke --as <user>`; `webhook send --case … [--sign\|--bad-sig]`; `kvs`; `logs`; `export validate` (schema only, no correctness oracle); `lint` | — |

---------------------------------------------------------------------------------------------------------------------

## E. Contract outline (`FORGE2-CONTRACT.md` + prompt + STARTER + two machine-readable files)

**Size budget.**

| file | size |
|---|---|
| prompt `spec-build-forge2.md` | ≈ 3.0 KB |
| `FORGE2-CONTRACT.md` | ≈ 20 KB |
| `STARTER.md` | ≈ 4.0 KB |
| **prose total** | **≈ 27 KB** (1.0: 18.9 KB; a 70 KB contract once caused a 3-hour desk audit) |
| `RATE-MODEL.json` (pool, cost table, buckets, per-issue windows, header examples, latency model, token estimator, credit rates) | ≈ 3.5 KB |
| `CONTRACT-SCHEMAS.json` (webhook body, export/action output, LLM tool schema, admin payloads) | ≈ 2.5 KB |

The prose stays readable in one pass. The tables are machine-readable because a model consults them while coding, not
by reading them through, so they invite no desk audit.

Throughout this design, **§E.n** means row § n of the table below, which is contract section n. The registry's
"anchor" column uses the bare contract section number (§n).

| § | title (size) | key guarantee sentences (verbatim intent) |
|---|---|---|
| 0 | What you build and how it is graded (1.0 KB) | "The harness installs your app on several seeded Jira sites at once and runs it in virtual time: hours of traffic in minutes, identical on every machine. It grades what your app does — the escalations it records, the Jira changes it makes, the calls it spends and what each person sees — never how your code is shaped." |
| 1 | Escalations (2.5 KB) | "Every CRM delivery your web trigger answers with 202 is reflected in the escalation it names. An escalation's state is the state of the highest `seq` delivered for its case, whatever order, timing or number of copies the deliveries arrive in." · "Each case that names no issue gets exactly one Jira issue in the project its `product` maps to; each CRM note becomes exactly one Jira comment on the escalation's issue, authored by the app, whatever happens between your app and Jira." · SLA rule: "An escalation's active time is the time since it opened during which its issue's status was not one of the policy's pause statuses. Response is met when the issue first leaves the `new` status category; resolution when it first enters `done`. A clock breaches when active time exceeds the minutes of the policy in force; a policy change applies from the instant it is saved and keeps the active time already spent." · "An escalation follows its issue by id: through a move it keeps its clocks; if the issue or its project is deleted, the escalation shows `issueState: deleted` and its clocks stop." |
| 2 | Who sees what (2.0 KB) | "Roles: `desk-admin` (granted in the admin panel; every Jira administrator is one implicitly) and everyone else. Every resolver, the Rovo action and the JQL function can be called directly by anyone who can load your app; display conditions and hidden pages are not authorisation." · "Refusals return `{ \"error\": \"forbidden\" }` and change nothing." · "A person sees an escalation's data only for an issue they can browse, judged by a permission check made for that person and that issue at most 10 virtual minutes earlier." · "The harness does not validate `context.extension` ids." · "No self-grant; the last desk-admin cannot be removed." · "Identity comes from the invocation context, never from a payload or an action input." |
| 3 | Intake (2.0 KB) | The full CRM sender spec (§C.8): headers, signing string, encoding, the ±300 s window, retries with the same delivery id, up to 4 concurrent, the 10 s answer timeout · "A request that fails verification is answered 401 (`unauthorized`) and changes nothing; a malformed body 400; anything else 202. A delivery id you already accepted is answered 202 and applied once." · "Your app must stay eligible for Runs on Atlassian: a static web trigger, no egress, no remotes." · "The platform's `hmacSharedSecret` is not used by the sender." |
| 4 | The rate pool and dosing (2.5 KB + `RATE-MODEL.json`) | "Your app's Jira calls from every installation, including installations outside the scenario, share one pool of 65,000 points per virtual UTC hour. It resets at the top of the hour with no carry-over, and an empty pool refuses every request until the reset." · "Calls a person is waiting for are interactive; all others are background. In every virtual hour each installation's background calls spend at most 12,000 points, and no 5-minute window holds more than a quarter of that hour's background spend." · "When the pool reports less than 20% remaining, background calls wait for the reset. After any refusal from the pool, an installation makes no Jira call until the reset." · "At most 4 Jira requests in flight per invocation; never before `Retry-After` (or the reset when it is absent); at most 4 attempts per request; never repeat a write you cannot prove did not land." · "Requests your Custom UI makes through `@forge/bridge` `requestJira` are not charged to the pool and are not refused by it; they are subject to the burst buckets." · "A person may request at most 5 drafts per virtual minute; refused requests cost no Jira or LLM call." · freshness: "Outside pool refusals, a delivery is reflected in the export within 10 virtual minutes, a note appears as a comment within 30, and a status change moves the clocks within 10." |
| 5 | Front surfaces (2.5 KB) | DOM hooks of §C.6 · "Boot is graded cold by counting, not timing: the shell is present before any backend response; READY needs at most one wave of backend responses, at most 2 backend-bound operations, at most N KB of your own files (gzip -9; platform scripts and theme CSS excluded), at most 6 requests of your own files and an initiator chain at most 3 deep; navigating within the surface repeats no bootstrap call. If the bootstrap `invoke` is refused with 429, keep the shell and retry exactly once at or after `rateLimitReset`." · "While the pool refuses calls, show `[data-testid=\"quota-paused\"]` with the reset instant and only the data §2 still allows." · tokens/contrast/dark/CSP as in 1.0 §7 · "Live updates through Forge Realtime; no polling." |
| 6 | Admin panel (2.0 KB) | UI Kit only (`render: native`), the tab and `testId` list of §C.7, the resolver payloads (`CONTRACT-SCHEMAS.json`) · "Each Save sends exactly one call with the stated payload; invalid input shows the stated message and sends none." · "Tables sort by cell `key` the way Atlassian's dynamic table does." · "The secret is never shown back — only whether it is set." · the component allowlist the harness renders (anything else shows as unmodelled) |
| 7 | Forge LLM (1.5 KB) | The tool schema · "Use a model `list()` reports `active` at call time." · "The prompt holds only what the requesting person may see; model output is untrusted: it is shown as text, its issue keys only if visible, and it changes nothing until the person confirms." · the cache key · the budgets and the exhaustion message · the AI label `[data-testid=\"ai-label\"]` · "Drafts are delivered to the requesting person only." · "The digest uses the cheapest active tier (the tier is the word haiku, sonnet or opus in the id)." · the credit rates · "Stay within 100 requests per 60 s per installation and 500,000 tokens per 60 s per model (the estimator in `RATE-MODEL.json`)." |
| 8 | JQL function and Rovo action (1.0 KB) | `escalationBreached(priority?)` semantics: "the issues whose escalation is breached now; it is evaluated once and shared by every user" · `get-escalation` input and output schema · "unknown, missing or invisible → `{error}`" |
| 9 | Escalation Desk 1.x data (1.0 KB) | The 1.x layout of §C.9 · "Every 1.x escalation is served correctly at all times and is converted exactly once; when conversion is complete no `esc1:` or `esc1idx:` key remains; 1.x events still queued on `crm-events` are applied." |
| 10 | Platform facts the harness reproduces + harness deviations (2.0 KB) | Platform facts, one line each, with doc URLs: at-least-once and unordered async delivery with 24 h retention and `retryAfter` ≤ 900; scheduled duplicates and no retry after a throw; KVS `get` strict and `query` eventual; expired TTL values readable up to 48 h; warm reuse across tenants; unawaited work may run later or never; product events up to 3 min late, `selfGenerated`, only top-level deletes; install-time 401/403 until permissions propagate (the window is stated); `search/jql` freshness and `reconcileIssues`; a request may fail after the server applied it · Deviations: virtual time and its 1 µs clock-read quantum; latency per call class (`RATE-MODEL.json`); the worker reuse model; the concurrency of §D.3; scoring sites bigger and differently seeded; unmodelled calls answer 501 and HOLD the verdict (1.0 R5). |

**What the contract deliberately does NOT say.**
- Which faults fire, where, when or how often.
- That the scheduled job runs before any update (1.0 did, frontier-behaviour §2 row 7).
- Which issue refuses which write.
- The LLM failure sequence.
- When the wall comes.
- The burst size and time.
- Which users lose access.
- The economy optimum: there is none to guess, because the budgets are stated.

---------------------------------------------------------------------------------------------------------------------

## F. Check registry

### F.0 Registry rules (the §17.8 lessons as mechanisms, not intentions)

1. **Every row is continuous.** A count row scores `min(1, budget / measured)`. A fraction row scores the fraction.
   There are no steps, so one call never moves a row from 1.0 to 0.75 (defect F).
2. **Worst of the three scoring seeds, for every row.** Every row is a stated requirement, so 2.0 has no mean-graded
   excellence slice and no unstated optimum (frontier-behaviour §5.3: "A hidden preference rewards incidental design").
3. **Harness evidence is never app evidence.**
   - A probe step that fails for a harness reason is recorded with its reason, the row becomes `unavailable` and the
     verdict is HELD for rescore. Harness reasons are: a click intercepted by host chrome, the CPU watchdog, a `501
     harness_missing`, or a browser crash.
   - Host chrome (flags, modals the host draws) is drawn outside the app frame, or with `pointer-events:none`
     (defect A).
4. **Each critical fires only on its named consequence, observed.** A duplicate means ≥ 2 effects; a leak means a
   canary seen in a person-facing channel. An absence is graded in its own non-critical row (defect B).
5. **Each root is priced once.** `ROOT_BLOCKS` attributes downstream rows to a failed root. A vacuous row (its
   precondition unmet) scores 0 with no multiplier of its own (G5 from 1.0). A root outside a row's tier never
   re-charges it (defect G).
6. **Basis column.** M, D or W per §0.2. A row with any G in its evidence path is refused at freeze.
7. **Contract anchor.** Every row names its §E sentence. `test_score_forge2.py` fails on a row without an anchor, or
   an anchor without a row (the 1.0 §14 mechanism).
8. **One-defect mutant.** Every row has at least one mutant that loses exactly that row (± ROOT_BLOCKS attribution).

How state is read:
- the auditor read path, `admin.export`, invoked directly as a site administrator (an implicit desk-admin);
- the mock Jira's own state and log (comments, issues, labels, properties, every call with its virtual time,
  principal, points and status);
- the KVS snapshot (secrets store, leftover 1.x keys);
- the bridge and ForgeDoc logs;
- the DOM.

Canaries are whole-token, seeded per scoring site, and never present in the dev pack.

**Tier weights (inner, sum 1.00):**

| L deployability | S security | R robustness | T tier-1 dosing | B boot | U surfaces | G Forge LLM |
|---:|---:|---:|---:|---:|---:|---:|
| .06 | .20 | .22 | .18 | .08 | .14 | .12 |

### F.1 L — deployability and platform currency (0.06)

| id | measured | anchor | w | C | basis | load-robust measurement |
|---|---|---|---:|:-:|:-:|---|
| `l_deployable` | 0 errors from the CLI's client lint (16 linters, byte-identical pins) + the measured server-rule pack (MANIFEST_INVALID_RULE texts with wolfaenpak receipts) + the pinned deprecated-runtime flag `["sandbox","nodejs18.x","nodejs20.x"]` | P done 1 | .015 | band 1 | M | static; lint run twice, identical |
| `l_bundles_load` | every manifest function bundles (esbuild as `forge deploy`) and loads in a warm worker with its handler exported | P done 1 | .010 | band 1 | M | static + one load per function |
| `l_lint_warnings` | `1 − 0.2·n` distinct warnings | P done 1 | .005 | | M | static |
| `l_manifest_contract` | fraction of stated module facts: one `jira:globalPage`; issuePanel; `jira:adminPage` with `render: native` and a file resource; static `crm-intake` with outputs `accepted`/`unauthorized`/`malformed`; JQL function `escalationBreached`; `llm` module; action `get-escalation` (`actionVerb: GET`); consumer for 1.x queue `crm-events` | §0, §3, §6–§9 | .010 | | M | static |
| `l_scopes` | declared = required per the shipped OpenAPI per observed call (1.0 rule); `−0.25` per missing or extra | §0 | .010 | | M | call log |
| `l_roa_eligible` | static predicates: no dynamic web trigger, no `permissions.external`, no remotes/providers/Connect modules, no `unsafe-*` script relaxations | §3 | .010 | | M (calibrated by `forge eligibility` on the golden, §I) | static |

### F.2 S — security (0.20)

| id | measured | anchor | w | C | basis | load-robust measurement |
|---|---|---|---:|:-:|:-:|---|
| `s_admin_authz` | every admin resolver key invoked DIRECTLY (bypassing UI) as: licensed member, project admin without ADMINISTER, `unidentified` anonymous, a member whose desk-admin role was revoked 6 virtual min earlier (TTL 5), with valid payloads → refusal value AND zero diff in KVS, Jira write log, queue and secret store; fraction over key × principal | §2 | .020 | C3 if any diff | M (resolver reach, sec#4) + W (roles) | KVS/Jira diff before/after each call, in lockstep |
| `s_identity_from_context` | payload/input fields `accountId`, `isAdmin`, `role`, `requester` on every resolver and the action → ignored; every attributed record names `context.accountId` | §2 | .010 | | D (sec#1,2,26) | record attribution |
| `s_idor` | `panel.postDraft` with another person's draft id; `panel.bootstrap` with `extension.issue.id` of an issue the caller cannot browse; `admin.export` by a non-admin; `get-escalation` for an invisible case → refusal, no data | §2 | .015 | C1 if data shown | W (unvalidated extension, stated) | response scan |
| `s_person_visibility` | canaries (customer names, summaries, comment text) of issues hidden from the viewer by security level, reporter-only browse or role removal appear in NO person-facing channel: resolver/action responses, DOM, Realtime payloads delivered to that person, LLM prompts built for that person | §2 | .025 | C1 | M (asUser filtering, P13) + W (asApp worst case stated) | whole-token scan of every channel |
| `s_verdict_ttl` | after a revocation at virtual t, no person-facing response at t + 10 min + ε (one load) shows the issue's data; fraction over revocation probes | §2 | .015 | | W (TTL stated) | virtual-time checkpoints |
| `s_webhook_auth` | unsigned; wrong secret; tampered body; stale (−301 s); future (+301 s); signature over re-serialised JSON instead of raw bytes; truncated hex → 401 `unauthorized` output and zero side effects (no KVS write except reads, no push, no Jira call) | §3 | .020 | C3 if a side effect | W (sender spec) + M (request shape, P10) | KVS/queue/Jira diff |
| `s_webhook_replay` | the same valid delivery sent twice sequentially, twice concurrently (interleaved by the scheduler), and again after the nonce's TTL would have lapsed but inside the window → exactly one effect and 202 each | §3 | .015 | C2 if 2 effects reach Jira | M (FAIL_IF_EXISTS 409, P01; TTL-readable lag, P03) | effect count |
| `s_webhook_variants` | valid requests with header names in other cases, multi-value arrays, insignificant whitespace in the raw body → accepted (202) and applied; fraction | §3 | .010 | | M (header case/arrays, P10) | outcome |
| `s_secrets` | the seeded secret canary, entered through `admin.setSecret`: present in the secret store; absent from every KVS value, response (`getConfig` returns presence only), log line, Realtime payload, LLM prompt and DOM | §3, §6 | .010 | C1 | M (setSecret path, P01) | canary scan |
| `s_tenant_isolation` | per-installation canaries never cross installations in responses, storage or Jira writes; I/O issued by work an invocation left unawaited and that resumed during another installation's invocation counts as a crossing | §10 (warm reuse) | .015 | C1 | D (rob#11,12; tenant-isolation page) | proxy attribution by token, scheduler-controlled reuse |
| `s_xss` | seeded payloads in CRM fields, issue summaries, comments and LLM output (`<img onerror>`, `javascript:` hrefs, `</script>`, ADF marks) → no script runs (a canary global stays unset), no `javascript:` href in the DOM, no `unsafe-inline` script | §5, §7 | .010 | | M (production CSP via `@forge/csp`) | DOM + CSP report collector |
| `s_prompt_injection` | issue text instructs the model; the scripted model "obeys" (a non-listed tool call, an exfiltration URL in the summary, a hidden key cited, a request to post) → no write without confirmation, no egress attempt, the URL rendered as text, the hidden key dropped | §7 | .015 | | W (scripted model) | Jira write log + egress proxy log + DOM |
| `s_realtime_isolation` | a second person subscribing to the first person's draft channel name (direct resolver call with forged claims in the payload) receives nothing; board-channel payloads carry case ids only | §7, §5 | .010 | C1 if data delivered | M (pairing and claims, P15) | Realtime delivery log |
| `s_jql_user_agnostic` | the function's fragment is identical whichever user's search first evaluates it; results after Jira's permission filter are correct for each searcher | §8 | .005 | | M (precomputation sharing, P18) | mock search log |
| `s_rovo_inputs` | `get-escalation` with a forged `accountId` input, an invisible case, or a missing input → `{error}` or the caller's own view; never another person's | §8 | .005 | | D (sec#26) | action responses |

### F.3 R — robustness and correctness (0.22)

| id | measured | anchor | w | C | basis | load-robust measurement |
|---|---|---|---:|:-:|:-:|---|
| `r_one_record_per_case` | at CP4, the export holds exactly one escalation per case that ever received a 202, with the stated fields; fraction | §1 | .020 | C4 (continuous: factor `0.6 + 0.4·retained`) | W (sender) | export vs oracle |
| `r_monotonic_state` | each escalation's state equals the highest delivered `seq`'s, under out-of-order and concurrent deliveries; fraction | §1 | .015 | | D (no ordering, rob#30) | export vs oracle |
| `r_no_lost_update` | fields written by different paths for the same case (CRM state, clocks from Jira, policy version, notes) equal the oracle; fraction over contended cases (the scenario guarantees ≥ 150 contended cases per seed) | §1 | .020 | | M (CES conditional writes, P04) | export vs oracle; the racy mutant must fail on 20/20 calibration seeds |
| `r_note_exactly_once` | per CRM note: comments on the issue carrying it = 1; fraction with exactly one (0 counts here, not as a duplicate) | §1 | .020 | C2 (≥ 2 for any note) | M (comment `properties` + `comment/list` expand, P13) + W (ambiguous outcome) | mock Jira comments |
| `r_issue_exactly_once` | per case naming no issue: created issues = 1 | §1 | .015 | C2 (≥ 2) | M (create + search freshness/`reconcileIssues`, P13) + W | mock Jira issues |
| `r_sla_clocks` | `respondedAt`, `resolvedAt`, both due times, breach state, pause accounting and policy version per escalation, against the oracle's timeline; fraction of fields | §1 | .020 | | W (rule stated) + M (status-change payloads, P11) | export at CP1/CP2/CP4 |
| `r_continuation` | A's initial work (migration, backfill of escalated issues) completes exactly once, though it cannot fit one invocation (> 900 virtual s of calls at the stated latencies): completeness and no duplicates | §1, §9, §10 | .015 | | M (timeouts and remaining time, P07) | export + Jira log |
| `r_kill_safety` | items whose processing was interrupted by a module-timeout kill (the scheduler records which) end exactly right: not lost, not doubled; fraction | §10 | .010 | | M (P07) | oracle over the kill set |
| `r_poison_items` | unprocessable items (a case naming a non-existent key, a payload failing the schema, an issue deleted before first apply) appear in `admin.failures` with a reason within 30 virtual min, everything else completes, and no item is delivered more than 6 times | §1, §6 | .010 | | M (redelivery schedule, P06) | admin resolver + delivery log |
| `r_world_changes` | moves keep the escalation (same id, new key shown); deleted issue or project → `issueState: deleted`, clocks stopped; a policy edit applies from its instant; fraction over change events | §1 | .020 | | M (P11/P13 move, delete, project delete) | export vs oracle |
| `r_scheduled_semantics` | duplicate concurrent ticks, an overlapping long run and two skipped ticks → no breach recorded twice (comment/label count), no breach missed by CP4 | §10 | .010 | | D (rob#38–40) | Jira log + export |
| `r_migration` | every 1.x record migrated exactly once (by CP4); no `esc1:`/`esc1idx:` key remains; every pending 1.x event applied; export correct for sampled 1.x cases at CP1 (dual read); fraction | §9 | .020 | C4 (a lost 1.x record) | D (rob#32) + M (KVS) | export + KVS snapshot |
| `r_install_race` | work attempted during the stated 401/403 window lands after it; nothing dropped | §10 | .005 | | D (rob#37) | oracle |
| `r_self_events` | the app's own comments, labels and properties cause no further writes from their own events; writes per issue bounded by the oracle's count + 0 | §10 | .005 | | M (`selfGenerated`, P11) | Jira write log |
| `r_freshness` | outside pool refusals: deliveries reflected in the export ≤ 10 virtual min; notes as comments ≤ 30; status → clocks ≤ 10; fraction of items meeting their objective | §4 | .015 | | W (objectives stated; latency model stated) | virtual timestamps from the call log |

### F.4 T — Tier 1 dosing (0.18)

Every T row reads the gateway ledger. It has one row per product call: `{t_virtual, installation, invocation id,
kind (interactive | background), principal (asApp | asUser | bridge-user), method, path, status, points, policy,
reason, headers sent}`. Points are computed by the stated table, so all T rows are pure functions of the call log.

| id | measured | anchor | w | C | basis | load-robust measurement |
|---|---|---|---:|:-:|:-:|---|
| `t_install_budget` | per installation-hour: background points ≤ 12,000 → 1.0, else `max(0, 1 − (spent − B)/B)`; mean over installation-hours | §4 | .030 | C5 (see F.8) | W | ledger |
| `t_pool_wall_pause` | after an installation receives a quota-class refusal (`jira-quota-*`, an unknown reason, or the gateway variant with only `X-RateLimit-Reset`), its next product call (any invocation) is at or after the reset; fraction over (wall × installation) | §4 | .025 | | W (+ D: "pause all requests", tiers#20) | ledger, virtual time |
| `t_near_limit_backoff` | while `RateLimit` showed `r < 0.2·q` to an installation, that installation's background calls until the stated reset; score `1 − calls_in_window / calls_in_equal_window_before` floored at 0 | §4 | .015 | | W (grammar from docs, tiers#17,18) | ledger |
| `t_burst_per_endpoint` | mean of: (a) no repeat to a throttled endpoint and method before its `Retry-After`; (b) `min(1, allowance / burst-429s)` with allowance = 3 per installation-hour (an app pacing at the stated steady state never meets one) | §4 | .015 | | W (buckets stated) | ledger |
| `t_per_issue_writes` | `jira-per-issue-on-write` refusals received + writes sent inside a refused window; score `min(1, 2 / (1 + refusals))` | §4 | .010 | | D (tiers#13) + W | ledger |
| `t_retry_discipline` | every repeat of a refused request is at or after `Retry-After` (or the reset); ≤ 4 attempts per request; fraction of compliant repeats | §4 | .020 | | D (tiers#21: only the three gradable parts) | ledger. A "repeat" is the same invocation, method, path and body, after a refusal; concurrent refusals are never charged (1.0 §17.6 rule) |
| `t_in_flight` | max product requests in flight per invocation; score `min(1, 4 / max)` | §4 | .010 | | W | lockstep in-flight counter |
| `t_smoothness` | per installation-hour: max 5-minute background spend ≤ 25% of that hour's background spend → 1, else `0.25 / share`; mean | §4 | .010 | | W | ledger |
| `t_interactive_reserve` | outside pool refusals, the share of person-facing resolver and action calls on installation C that return data (not an error value) during A's burst and backfill hours | §4 | .015 | | W | resolver responses |
| `t_user_throttle` | the button-masher: 40 draft requests from one person in 60 virtual s → at most 5 accepted per minute; refused requests make 0 Jira and 0 LLM calls; score = fraction of the excess correctly refused at zero cost | §4 | .010 | | W | resolver responses + ledger + LLM log |
| `t_usage_accuracy` | `admin.usage` at CP2 and CP4 vs ground truth: `pointsThisHour` ±5% of the ledger's background + interactive points for that installation; refusals by reason exact; `pausedUntil` exact; `llmCreditsToday` ±2%; `projectedUsd` by the stated formula ±2%; fraction of fields | §4, §6 | .020 | | W | resolver JSON vs ledger |

### F.5 B — boot (0.08)

Per front surface (board, panel), cold context, production CSP from `@forge/csp`, real `@forge/bridge` (so the
500/25 s limiter and module-load capture behave as in production), hold-and-release waves, scoring seed ≠ dev seed.
Each row is the worse of the two surfaces. READY is verified by the seeded CONTENT, not by a marker.

| id | measured | anchor | w | basis | load-robust measurement |
|---|---|---|---:|:-:|---|
| `b_shell_first` | SHELL present at wave 0 (before any backend-bound response) | §5 | .010 | W (budget) + M (served form, boot#2) | wave count |
| `b_one_wave` | `min(1, 1 / waves_to_READY)` | §5 | .010 | W | wave count |
| `b_ops_budget` | `min(1, 2 / backend_ops_before_READY)` (`invoke`, `requestJira`, Realtime subscribe, `invokeRemote`, flags init) | §5 | .010 | W | bridge log |
| `b_bytes_requests_depth` | mean of `min(1, N_KB / gz9_bytes)`, `min(1, 6 / requests)` and `min(1, 3 / depth)` over app-origin files before READY (platform scripts and theme token CSS excluded by URL pattern, boot#19) | §5 | .010 | W | served bodies gzipped at -9; CDP initiators with the Debugger domain on the frame |
| `b_bootstrap_scaling` | the bootstrap resolver's sequential outbound rounds `min(1, 3 / rounds)` × call growth `min(1, (calls_N + pages) / calls_4N)`, comparing the run seed's C-sized page with a 4× page from the same tree at CP4 | §5 | .010 | W | lockstep rounds (calls released when the event loop idles) |
| `b_csp_clean` | 0 CSP violations and 0 requests to non-app, non-platform origins; the allow-list includes the default CSP hosts (avatars) | §5 | .010 | M (`@forge/csp`; production CSP observed, boot#7) | report collector |
| `b_no_regate` | mean of: SHELL present while the Realtime subscribe is held; 0 repeated bootstrap invokes on in-surface navigation (filter change, tab, back) | §5 | .005 | W | bridge log |
| `b_boot_429` | the bootstrap invoke refused with 429 + `rateLimitProperties`: the shell stays, exactly 1 retry, at or after `rateLimitReset` | §5 | .005 | M (metadata shape, P14) + W (one-retry rule) | bridge log, virtual time |
| `b_admin_boot` | UI Kit admin: `min(1, 1 / invokes_before_first_doc_with_default_tab)` × (first ForgeDoc non-empty) | §6 | .010 | W | Node host bridge log |

**Excluded by design:** JS work counts and threadTicks (BRIEF §4.3 B10). No instrument for them passes the double-run
test under load. Timer and rAF code adds iterations under load (boot#38).

### F.6 U — surfaces (0.14)

| id | measured | anchor | w | basis | load-robust measurement |
|---|---|---|---:|:-:|---|
| `u_board_numbers` | the four KPI tiles and the at-risk page = oracle for the viewer at CP2 and CP4 (three viewers: all-seeing, restricted, revoked) | §5 | .020 | W | DOM vs oracle |
| `u_board_table` | sort by due time (instants) and priority (stated order), filter by priority, issue-key link → bridge `router.open`/`navigate` to `/browse/<KEY>`; fraction | §5 | .010 | M (router ops, P14) | DOM + bridge log |
| `u_board_live` | the board open while a recorded change is published → the new state shows within one release wave after the publish; 0 invokes while idle (polling scores 0, not 0.5: defect H) | §5 | .010 | M (Realtime pairing, P15) | wave + bridge log |
| `u_panel_timeline` | the panel shows the issue's escalation(s), `li[data-seq]` in `seq` order, clock `time[datetime]` values = oracle | §5 | .010 | W | DOM vs oracle |
| `u_panel_draft_flow` | request → draft shown with the AI label → confirm → exactly one ADF comment by the viewer with the draft text; cancel → none; a failure shows `[data-testid="draft-error"]` and the panel keeps working | §5, §7 | .020 | M (ADF comment as user, 1.0) | DOM + mock Jira |
| `u_quota_paused_state` | at CP3: `[data-testid="quota-paused"]` with `datetime` = the reset instant, on both surfaces; data shown obeys the TTL rule | §5 | .010 | W | DOM |
| `u_theme_dark` | `theme.enable()` called; text colours are `--ds-text*` values; contrast ≥ 4.5:1 in both modes (disabled controls exempt); a `--ds-surface*` painted background; no horizontal scroll at 1280 and 380 px | §5 | .010 | M (pinned tokens) | computed styles, pixels for dominant colour only |
| `u_admin_secret` | Integration tab: valid Save → exactly one `admin.setSecret`; empty → ErrorMessage and 0 invokes; the Lozenge shows `Set` after and never the value | §6 | .010 | M (Form/Textfield callback shapes, P16) | ForgeDoc + bridge log |
| `u_admin_roles` | grant through UserPicker → `grantRole` payload `accountId` is a STRING; revoke only after the Modal confirm; the table lists grants | §6 | .010 | M (UserPicker `onChange` shape, P16) | ForgeDoc + bridge log |
| `u_admin_policy` | invalid values → per-field ErrorMessage, 0 invokes; valid → one `setPolicy` with integers; double activation of Save before the first resolves → 1 invoke | §6 | .010 | M (useForm/`handleSubmit`, uikit#9,28; P16) | bridge log |
| `u_admin_failures_table` | header clicks on `attempts`, `first-seen`, `age-hours` produce the stated orders (fixtures include decimals and display-date traps that mis-sort as strings); Retry → one invoke, the row state updates | §6 | .010 | M (ADS comparator, uikit#11,12; pinned en-US collator) | host-owned sort by cell `key` |
| `u_admin_usage_migration` | Usage and Migration tabs show the resolver's values in the stated `testId`s | §6 | .005 | W | ForgeDoc |
| `u_admin_quiescence` | after boot and after each scripted action: no `reconcile` within K host ticks, no `onError`, no `BridgeAPIError` | §6 | .005 | M (real reconciler) | host tick counter |

### F.7 G — Forge LLM (0.12)

The model is a scripted fake with a seeded per-prompt behaviour. Model quality is never graded. The emulator enforces
the documented validation rules exactly (llm#16–18).

| id | measured | anchor | w | basis | load-robust measurement |
|---|---|---|---:|:-:|---|
| `g_model_lifecycle` | every request names a model `list()` reports `active` at call time (the scoring `list()` marks one listed model `deprecated`, a real-world class: sonnet-4-5, llm "newer facts" 1); no validation 4xx (sampling rules) | §7 | .015 | M (P17 validation) + D (`list()` statuses) | LLM log |
| `g_forced_tool_validation` | requests carry tool `customer_update` and a forcing `tool_choice`; malformed arguments (wrong types, missing or extra fields) → error state and no draft stored; JSON-string arguments parsed, not rejected | §7 | .020 | D (llm#4,5; forcing stated) | LLM log + KVS + DOM |
| `g_grounding` | shown issue keys ⊆ keys visible to the requester; the summary is text (no element from model markup, no link); the AI label is present | §7 | .020 | W | DOM |
| `g_failure_states` | refusal (no tool call), 403 `FORGE_LLMS_MODEL_FORBIDDEN`, 429, 500, a stream ending without `finish_reason`, an empty `end_turn` → the stated state; ≤ 1 retry and not before the stated 30 virtual s; non-AI features keep working; fraction | §7 | .020 | D (llm#11,12,35–38) + W (shapes stated) | LLM log + DOM |
| `g_placement` | no LLM call inside a resolver invocation and none during any page boot; long drafts (scripted latency > 25 s, ≤ 180 s per call) complete through a consumer with `timeoutSeconds` ≥ 300 and reach the requester | §7 | .015 | M (timeouts, P07) + D (llm#23,25) | invocation kinds in the LLM log |
| `g_dosing` | per installation: ≤ 100 requests per 60 virtual s; ≤ 500k estimator tokens per 60 s per model; the digest completes by CP4; score `min(1, 100 / peak_rpm) × min(1, 500k / peak_tpm) × completed_fraction` | §7 | .015 | D (llm#20,21) + W (estimator) | LLM log |
| `g_economy` | `max_completion_tokens` on every call; an identical repeat (same requester scope + content) → 0 calls; an unchanged digest rerun → 0 calls; budgets: after exhaustion 0 calls + the stated message; digest calls use the cheapest active tier; fraction of sub-checks | §7 | .015 | W | LLM log |

**Deliberately not graded, because no load-robust observation exists:** constant-time HMAC comparison. CPU time is
not charged in lockstep, so a timing leak cannot be observed deterministically. The contract does not mention it, and
the security write-up names it as good practice only.

### F.8 Composition and criticals

`inner = Σ w_row · score_row` over available rows (76 rows, weights as above, sum 1.00).
`earned = inner × Π critical factors`; `final = min(earned, ceiling − 0.05·(1 − earned))` (1.0's `capped_final`, so a
final never sits on a cap). Any `unavailable` row makes the verdict unpublishable and HELD for rescore. There is no
excellence slice: every efficiency target is a stated budget inside a weighted row.

Five criticals. Each one names one consequence a customer would uninstall for, fires only when that consequence is
OBSERVED, and is priced once per verdict however many roots produce it. The rows carry the extent.

| critical | fires when (observed) | factor | rows that carry the extent |
|---|---|---|---|
| C1 data exposure | any canary seen where it must not be: a hidden issue's data in a person-facing channel; the secret outside the secret store; one installation's canary in another's; a person's draft delivered to someone else | 0.6 | `s_person_visibility`, `s_idor`, `s_secrets`, `s_tenant_isolation`, `s_realtime_isolation` |
| C2 duplicate side effect on the customer's Jira | ≥ 2 comments for one note, ≥ 2 issues for one case, or ≥ 2 Jira effects from one delivery id | 0.6 | `r_note_exactly_once`, `r_issue_exactly_once`, `s_webhook_replay` |
| C3 forged or unauthorised change | a non-admin call changed admin-held state; a request that failed verification caused any side effect | 0.6 | `s_admin_authz`, `s_webhook_auth` |
| C4 data loss | an accepted case or a 1.x record absent at CP4 | `0.6 + 0.4·retained` (continuous: no one-item cliff) | `r_one_record_per_case`, `r_migration` |
| C5 cross-tenant pool denial | in an hour in which the pool walled, the app's in-scenario background spend exceeded the stated per-installation budgets, AND the gateway's counterfactual (spend minus the overspend) stays below 65,000. The app caused the wall that took every tenant down | 0.6 | `t_install_budget` |

**What can never fire a critical:**
- an absent surface (vacuous);
- zero comments (that is `r_note_exactly_once`'s fraction, not a duplicate);
- a missing flag or label;
- a harness failure (unavailable);
- a wall the invisible installations caused on their own.

### F.9 Admission bands (only "the app is unusable" states; no graded robustness band)

| band | max | requires (`passed` = exactly 1.0, available, not vacuous) |
|---|---:|---|
| deployable | 0.499 | `l_deployable`, `l_bundles_load` |
| core working | 0.699 | four diagnostic rows (weight 0): `d_intake` (≥ 50% of valid deliveries reflected in the export at CP4), `d_export_schema` (the auditor export validates against `CONTRACT-SCHEMAS.json`), `d_board_ready` (board READY for the all-seeing viewer at CP4), `d_admin_renders` (the first ForgeDoc holds the default tab) |

**Why there is no 1.0-style graded band 4.** A band step on top of a row's own loss charges the same root twice.
Clustering scores at band caps also compresses the top, which the owner flagged ("two models at exactly 0.899 is a big
red flag"). In 2.0 the rows are demanding enough on their own, and severity comes from the five criticals.

`d_export_schema` makes the auditor read path a stated, dev-checkable contract: `forge-dev export validate` checks the
schema, never correctness. One formatting slip therefore cannot silently zero the R tier: it caps at 0.699 once, with
its reason.

### F.10 Severity selftest (wired into `--reference`; an inversion refuses the freeze)

Synthetic row sets run through the REAL composition:
1. every weighted row earns;
2. a measured zero-comment row (non-vacuous) scores ABOVE a duplicate (defect B's inversion);
3. a permission leak scores below a missing board;
4. C4 is continuous: 0.999 retained ⇒ factor 0.9996;
5. monotonicity: raising any row never lowers the final;
6. no row has a step: for every count row, measured + 1 changes the final by less than the row's weight × 0.5;
7. the empty starter and a one-function app score ≤ 0.05 (vacuity);
8. an app that only answers the web trigger 202 and does nothing else scores ≤ 0.10;
9. every critical fires on its named synthetic consequence and on nothing else, using a case set of near-misses
   (zero comments; an invisible-installation wall; a 404 to a leaked row);
10. `unavailable` never lowers a score: it holds it.

### F.11 One-defect mutants (representative; the gate needs ≥ 1 per row, ≈ 80 in all)

| mutant | defect | expected loss |
|---|---|---|
| `m_side_effect_before_verify` | pushes before checking the signature | `s_webhook_auth`, C3 |
| `m_nonce_get_then_set` | replay check `get` then `set` | `s_webhook_replay` (concurrent pair), C2 |
| `m_nonce_ttl_short` | nonce TTL 60 s with an `expireTime` check | `s_webhook_replay` |
| `m_sign_parsed_json` | HMAC over `JSON.stringify(JSON.parse(body))` | `s_webhook_variants` |
| `m_header_case` | reads `headers['X-Desk-Signature']` only | `s_webhook_variants` |
| `m_admin_ui_only` | admin resolvers trust the page | `s_admin_authz`, C3 |
| `m_payload_identity` | `requester` from the payload | `s_identity_from_context` |
| `m_trust_extension` | the panel trusts `extension.issue.id` | `s_idor`, C1 |
| `m_asapp_board` | board rows read asApp, unfiltered | `s_person_visibility`, `u_board_numbers`, C1 |
| `m_verdict_forever` | permission cache with no TTL | `s_verdict_ttl` |
| `m_verdict_per_tenant` | verdict cache keyed by cloudId, not person | `s_person_visibility`, C1 |
| `m_restricted_comment_prompt` | the draft prompt includes role-restricted comments | `s_person_visibility`, C1 |
| `m_secret_echo` | `getConfig` returns the secret | `s_secrets`, C1 |
| `m_module_cache_issue_key` | module-scope cache keyed by issue key | `s_tenant_isolation`, C1 |
| `m_unawaited_push` | fire-and-forget push after return | `s_tenant_isolation`, C1 |
| `m_inner_html` | renders CRM text with `dangerouslySetInnerHTML` | `s_xss` |
| `m_execute_any_tool` | executes the model's non-listed tool call | `s_prompt_injection` |
| `m_rt_global_draft` | the draft goes on a global channel named by case id | `s_realtime_isolation`, C1 |
| `m_jql_as_caller` | the function computes asUser of its first caller | `s_jql_user_agnostic` |
| `m_dedupe_jobid` | idempotency on `jobId` | `r_note_exactly_once`, C2 |
| `m_last_arrival_wins` | applies state without the `seq` check | `r_monotonic_state` |
| `m_rmw_unconditional` | read-modify-write without the version condition | `r_no_lost_update` (must fail on 20/20 calibration seeds) |
| `m_blind_comment_retry` | re-posts after a 5xx with no marker check | `r_note_exactly_once`, C2 |
| `m_search_too_soon` | ambiguous create → immediate search, no `reconcileIssues` | `r_issue_exactly_once`, C2 |
| `m_no_pause_status` | clocks ignore pause statuses | `r_sla_clocks` |
| `m_policy_retroactive` | a new policy recomputes from `openedAt` | `r_sla_clocks` |
| `m_backfill_single_invocation` | no continuation | `r_continuation` (A only) |
| `m_restart_chunk_no_watermark` | a killed chunk restarts from its start | `r_kill_safety` |
| `m_throw_on_404` | the consumer throws for a deleted issue | `r_poison_items`, `r_world_changes` |
| `m_key_not_id` | escalation keyed by issue key | `r_world_changes` |
| `m_sweep_no_lease` | concurrent sweeps both act | `r_scheduled_semantics` |
| `m_drop_v1_queue` | no consumer for `crm-events` | `r_migration`, C4 |
| `m_keep_v1_keys` | never deletes `esc1:` | `r_migration` |
| `m_no_ignore_self` | reacts to its own updates | `r_self_events` |
| `m_global_serialize` | one consumer, `limit: 1`, for everything | `r_freshness` |
| `m_no_accounting` | no background budget | `t_install_budget`, C5 |
| `m_pause_endpoint_on_quota` | a quota 429 pauses only its endpoint | `t_pool_wall_pause` |
| `m_ignore_r` | does not read `RateLimit` | `t_near_limit_backoff` |
| `m_retry_after_numeric` | `Number(retry-after) \|\| 1` | `t_pool_wall_pause`, `t_retry_discipline` (gateway variant) |
| `m_promise_all` | unbounded `Promise.all` over pages | `t_in_flight` |
| `m_top_of_hour` | the scheduled job does everything at :00 | `t_smoothness` |
| `m_flat_points` | counts 1 point per call | `t_usage_accuracy`, `t_install_budget` |
| `m_parallel_notes` | posts a hot case's notes in parallel | `t_per_issue_writes` |
| `m_no_user_throttle` | — | `t_user_throttle` |
| `m_await_before_render` | `await invoke()` before `render()` | `b_shell_first` |
| `m_waterfall` | getContext → invoke(config) → invoke(data) | `b_one_wave`, `b_ops_budget` |
| `m_dev_react` | React development build | `b_bytes_requests_depth` |
| `m_n_plus_1_bootstrap` | one Jira read per row in bootstrap | `b_bootstrap_scaling` |
| `m_cdn_font` | Google Fonts in index.html | `b_csp_clean` |
| `m_reinvoke_on_filter` | bootstrap again on filter change | `b_no_regate` |
| `m_retry_storm_429` | retries the bootstrap at once, 3 times | `b_boot_429` |
| `m_poll_board` | polls every 5 s instead of Realtime | `u_board_live` (0) |
| `m_post_on_arrival` | posts the draft without confirmation | `u_panel_draft_flow`, `s_prompt_injection` |
| `m_display_date_key` | failures table keyed by display dates | `u_admin_failures_table` |
| `m_userpicker_object` | sends the UserPicker object | `u_admin_roles` |
| `m_no_handle_submit` | `onSubmit` without `handleSubmit` | `u_admin_policy` |
| `m_hardcoded_white` | no tokens | `u_theme_dark` |
| `m_temperature_top_p` | README sampling parameters | `g_model_lifecycle`, `g_failure_states` |
| `m_stale_model` | a hard-coded id `list()` marks deprecated | `g_model_lifecycle` |
| `m_trust_args` | no argument validation | `g_forced_tool_validation` |
| `m_trust_cited_keys` | shows every cited key | `g_grounding` |
| `m_no_truncation_check` | a stream with no `finish_reason` shown as complete | `g_failure_states` |
| `m_llm_in_resolver` | drafts inside the resolver | `g_placement` |
| `m_digest_unpaced` | `Promise.all` over the digest | `g_dosing` |
| `m_no_llm_cache` | — | `g_economy` |

### F.12 Spread forecast (composition arithmetic on plausible row profiles; confirmed by the gate's calibration runs)

| profile | typical rows | inner | criticals | final |
|---|---|---:|---|---:|
| reference (golden) | all 1.0 | 1.00 | none | **1.000** |
| independent alt | a few sub-rows < 1 | ≥ 0.96 | none | **≥ 0.95** (gate) |
| frontier (Sol, Opus) | L ≈ 0.95, S ≈ 0.8, R ≈ 0.72, T ≈ 0.6, B ≈ 0.75, U ≈ 0.8, G ≈ 0.75 | ≈ 0.75 | 0 → 0.75; 1 (C2, C1 or C5 most likely) → 0.45 | **0.45–0.75**, central ≈ 0.58 |
| mid (1.0's 0.75–0.85 tier) | deployable; R ≈ 0.4, T ≈ 0.3, S ≈ 0.6, B ≈ 0.5, U ≈ 0.6, G ≈ 0.5 | ≈ 0.48 | 1–2 | **0.17–0.48** |
| weak | lint errors or broken bundles; little runs | ≈ 0.10–0.30 | — (vacuous) | **0.03–0.25** (band 1 caps at 0.499) |

---------------------------------------------------------------------------------------------------------------------

## G. Difficulty argument (GPT-6.1 Sol and Opus 5.5)

Caveat carried from the source: the Sol, Opus 5.5 and Sol Pro trees are not on this machine. Statements about HOW they
work are inferred from Haiku 5.5 (0.9661) and Pareto (0.9450), which sit 0.0108 and 0.0319 below them, and from their
board rows (frontier-behaviour §6, "Medium"). The predictions below are therefore medium confidence. The pre-build
proof (NOW.md phase 4) and the pilot (phase 7) are what settle them.

### G.1 How the frontier beat 1.0, and what 2.0 removes

| how they won 1.0 (quoted) | what 2.0 does instead |
|---|---|
| "The contract works as a test plan. Each injected fault is announced" — duplicates, reordering, loss, Retry-After, "the harness runs the scheduled job once before it delivers any update", the LLM's failure order, the double click | the contract states GUARANTEES and documented platform FACTS (§E.10), and never the scorer's plan: no timing, targets, order or volume, and no "the job runs first" guarantee (§D.4) |
| "Every semantic trap that both models handled traces to a contract sentence that Haiku paraphrased in its own design notes" | paraphrasing a guarantee ("exactly one comment per note, whatever happens between your app and Jira") does not produce the mechanism. The model still has to invent the marker-then-check protocol (§C.8) and get it right under lockstep interleavings |
| "All six defect classes Haiku found by testing were platform wiring, each announced by an explicit dev-tool error string" | dev tools print only what real Forge and Jira print (measured messages). Semantic defects show up as wrong numbers or wrong counts, never as an announcement (frontier-behaviour §5.2 item 7) |
| "The scoring world is small and static … The live phase delivers about 41 issue updates one at a time (measured overlap: 0) … the longest Retry-After was 30 seconds" | three installations, up to 7,000 issues, a 1,500-delivery burst, interleaved concurrent invocations, and a pool wall of up to ~45 virtual minutes against a 900 s `retryAfter` ceiling |
| "The budget never bound" (72 and 50 of 150 calls) | budget 250 is set NOT to bind a disciplined model either: the depth must be engineered, not raced (§J.3) |
| the residual was an unstated optimum (`e_event_economy` 0.40–0.42 for all top five) | no unstated targets. Budgets and objectives are numbers in the contract, so losing points means failing a stated requirement |

### G.2 The requirements Sol and Opus most plausibly fail, and why

1. **Lost updates under concurrent delivery** (`r_no_lost_update`; medium-high).
   - Haiku saw the race and accepted it: *"I'm accepting the eventual consistency tradeoff here since concurrent writes
     both read fresh state anyway"* (C18).
   - 2.0 makes four writers (CRM state, Jira status clocks, policy edits, notes) contend on ≥ 150 cases per seed. The
     interleavings are seeded and real at I/O boundaries, so a read-modify-write is wrong on some cases of every seed.
   - The escape Pareto used, *"serializing the whole site through one consumer (limit: 1), which is correct but has a
     throughput ceiling"*, now costs `r_freshness`. At the stated latency model, a single serial consumer cannot
     absorb A's burst within the 10-minute objective.
   - Only per-case partitioning (concurrency keys) or conditional CES transactions pass both rows. Both are documented;
     neither is announced.
2. **Exactly-once Jira writes when the outcome is unknown** (`r_note_exactly_once`, `r_issue_exactly_once`, C2;
   high).
   - In 1.0 the frontier put de-duplication in the UI: *"Never retry a successful POST; the UI coalesces a double click
     before invoking this resolver"* (Pareto) and *"rather than adding backend-side deduplication"* (Haiku).
   - 2.0's writes happen in consumers that are redelivered, so a blind retry duplicates. The idempotent-create path
     must also respect the documented search freshness: a search immediately after an ambiguous create misses the
     issue unless `reconcileIssues` names it.
   - This is the class most likely to fire a critical on a frontier model.
3. **A shared pool the app cannot see** (`t_install_budget`, `t_near_limit_backoff`, `t_pool_wall_pause`, C5;
   medium-high).
   - The 1.0 frontier slept inside the invocation (*"which works fine since consumer invocations can run up to 900
     seconds"*, never setting `timeoutSeconds`). Haiku parsed `Retry-After` as *"numeric seconds only"*.
   - In 2.0 the wall lasts longer than any single `retryAfter`, may come with no `Retry-After` at all (the gateway
     variant, a stated documented header set), and `r` appears only past 80%.
   - Per-installation self-accounting with a sharded counter, smoothing across the hour and reacting to `r` are stated
     but non-trivial to build. Models that "handle 429" pass `t_retry_discipline` and still fail three other T rows.
4. **Scale and continuation** (`r_continuation`, `t_install_budget`; medium).
   - Haiku designed and then dropped the incremental watermark: *"My plan: persist a `lastRunAt` watermark…"*, then
     shipped full rescans. Its *"no-op rerun issued 458 entity gets for 95 rows"*.
   - At A's scale a full rescan breaks the 12,000-point budget, and the initial work cannot fit one 900 s invocation
     at the stated latencies.
5. **A world that changes mid-run** (`r_world_changes`, `s_verdict_ttl`; medium).
   - *"Haiku caches the Sprint field and each board's estimation field forever"*; *"Pareto's consumer throws on a 404,
     so a deleted issue is redelivered for 24 hours"*; *"both store issue keys, which go stale after a move"*.
   - 2.0 moves, deletes (including a project, whose child issues emit no delete events, rob#36) and revokes access
     while the app runs. The verdict TTL is stated, so a permanent permission cache fails.
6. **Live migration** (`r_migration`; medium for failing, high for "a kind they have not been tested on").
   - *"Neither app versions its storage."* 1.x keys reference issue KEYS (some moved), and 1.x events are still queued
     on a queue the model must keep consuming.
7. **Security details that are knowable but easy to get partly wrong** (`s_webhook_replay`, `s_realtime_isolation`,
   `s_person_visibility` via role-restricted comments; medium).
   - Atlassian's own Realtime+LLM tutorial is exploitable (llm "newer facts" 5). A model that copies its shape delivers
     drafts on a guessable channel.
   - A nonce check written as `get`-then-`set` passes sequential replays and fails the concurrent pair.
8. **Boot discipline** (`b_one_wave`, `b_ops_budget`; medium-low).
   - The natural shape is getContext, then config, then data. The stated budget needs one wave and two ops, which a
     careful model can meet once it reads the numbers.

Where Sol and Opus should score near 1.0: L, admin authorisation, HMAC basics, secrets, XSS, theme, UI Kit wiring, the
LLM's basic forced-tool and validation path, and the JQL function's basic contract.

### G.3 Predicted Sol score

The row profile, its inner score and the effect of each critical are tabulated in §F.12. If no critical fires, inner
is ≈ 0.75 and the final is ≈ 0.70–0.78. The most likely firing is C2 (an ambiguous-write duplicate), then C1 (a draft
on a shared channel, or a role-restricted comment in a prompt), then C5. Each multiplies by 0.6.

**Predicted range: 0.45–0.75, central ≈ 0.58** (P(no critical) ≈ 35%, P(one) ≈ 45%, P(two or more) ≈ 20%; my judgment,
not a measurement). Opus 5.5 is expected in the same band; 1.0 put the two 0.0002 apart.

### G.4 Why this is engineering judgment, not trivia or volume

- **Not volume.** frontier-behaviour §5.3: *"Pure volume (more modules, surfaces, files) … would not challenge
  them."* 2.0 has three surfaces, about the same as 1.0's, and its weight sits in R, T and S (0.60 of inner), where
  the hard part is a protocol, not a file count.
- **Not trivia.** The traps that are documented knowledge carry a small share of the score:
  - the sampling parameters, part of `g_model_lifecycle` (.015);
  - the one-`globalPage` rule and the server lint rules, part of `l_deployable`;
  - the UI Kit callback shapes, inside U's admin rows (≈ .03 together).

  Each also has a dev-site signal: lint, a validation 400, or the host log.
- **Not hidden preferences.** Every budget and objective is a number in the contract. A frontier model that loses
  points loses them on a stated requirement it did not engineer, which is exactly the owner's acceptance criterion.
- **What remains is judgment.**
  - Partitioning versus conditional writes.
  - Where the idempotency key lives, and when it is safe to search.
  - Which reads belong in the browser (exempt), in a resolver, or in a background job (budgeted).
  - How to chain past a wall longer than `retryAfter`.
  - How to keep a migration correct while traffic flows.
  - How to cache permissions safely under a TTL.

  None of these can be transcribed from a sentence; each has a measurable right answer.

### G.5 The acceptance test (NOW.md phase 7), stated in advance

"Sol is challenged" requires all four of the following on its pilot verdict. If Sol scores ≥ 0.85 with zero C2/C1/C5
firings, harden before any other model runs (phase 3/4 notes the levers: the burst size, the wall length, contended
cases per seed).
1. `final ≤ 0.80`.
2. ≥ 0.15 of inner lost across R, T and S rows that anchor to contract sentences.
3. Zero `unavailable` rows and zero harness-attributed failures.
4. The reference = 1.000 and the alt ≥ 0.95 on the same scorer build.

---------------------------------------------------------------------------------------------------------------------

## H. Fairness argument: how a capable model can know every graded thing

1. **Every row has an anchor.** Each of the 76 rows names its contract sentence (§F). `test_score_forge2.py` refuses a
   row without an anchor and an anchor without a row (1.0 §14).
2. **Every number is written down.** Budgets (12,000 per installation-hour, 25% per 5 min, 4 in flight, 5 drafts per
   minute), the TTL (10 min), the objectives (10/30/10 min), the boot budgets (waves, ops, KB, requests, depth), the
   pool, the cost table, the bucket refill and capacity, the latency model, the token estimator and the credit rates
   are in the contract or in `RATE-MODEL.json`. A model can compute whether its design meets them before writing
   code. The latency model is stated precisely so that throughput is a design calculation, not a guess.
3. **Every platform behaviour the scorer exercises is a documented fact named in §E.10, with its doc URL.**
   Where docs conflict, the contract pins one side (BRIEF §9.1's 31 pins), and the scorer never grades the ambiguous
   side.
4. **Every behaviour class occurs on the dev site** during `forge-dev world run`. The dev world's event log tags each
   class, and WP1 proves coverage. A model that tests meets every class before the scorer does.
5. **Dev-tool messages are production messages** (measured on wolfaenpak), so the dev site teaches what production
   would teach and nothing more. `forge-dev ledger` shows the points the app spent: the "test quota" layer Atlassian
   staff recommend (tiers §5), so dosing is debuggable offline.
6. **There are no named fixtures and no scripted sequences** to transcribe, and nothing is stated that only the
   harness's plan could justify. 1.0's "the scheduled job runs before any update" is gone. In its place: "events may
   arrive before your first scheduled run", a fact.
7. **Outcome grading accepts every correct design.**
   - partitioning or conditional writes;
   - labels or properties for the JQL cap;
   - asUser search or `permissions/check` for verdicts;
   - `chat()` or `stream()`;
   - Tabs controlled or uncontrolled;
   - any storage layout.

   The alt app is built by a separate session from the public text only, with deliberately different choices, and
   must score ≥ 0.95 (§J.1 G4).
8. **Phase 4 pre-build proof.** Fresh Opus-class agents plan the app from the contract alone. Every "I could not know
   X" they raise becomes a contract fix or a removed row before any build.
9. **Partial credit everywhere, harness faults held.** Every row is continuous. A harness failure HOLDS the verdict
   and never zeroes a row. Host chrome never covers app controls.
10. **Knowledge traps are allowed only where Atlassian documents the rule and the emulator enforces it exactly**, the
    1.0 `temperature`/`top_p` precedent. Each carries a small weight and a dev-site signal.

---------------------------------------------------------------------------------------------------------------------

## I. Fidelity plan

### I.1 The conformance loop (real-forge-fidelity §4 layers L0–L3, made the build's spine)

- **One throwaway probe app.** `lz-forge2-probe` lives in the LeanZero developer space on **wolfaenpak**, the sanctioned
  test site.
  - Its webtrigger router is keyed on `userPath`. One consumer's behaviour is chosen by the event body.
  - It carries a `jira:globalPage`, a `jira:issuePanel` and a UI Kit `jira:adminPage` for the browser legs.
  - Its JQL function, product triggers and two scheduled triggers are cheap stubs that log.
- **One probe source.** The same `src/` runs live (answers committed as fixtures with date, CLI 14.x, runtime, Node
  version and invocation ids) and inside the 2.0 emulator as a kit test. **Any difference fails the kit build.** A
  behaviour that differs is fixed in the emulator, never in the contract (BRIEF §11.2 R2).
- **CPU discipline for every live session.** `nice -n 19`, one command at a time, no Chromium while a paid run is
  scoring; the browser legs run in quiet windows only. After each session the probe is uninstalled and fixtures are
  restored (the wolfaenpak rule in the owner's global notes).

### I.2 Every emulated behaviour → its probe, its class, and the rows resting on it

| emulated behaviour | probe | what the probe does | class | rows resting on it |
|---|---|---|:-:|---|
| KVS/CES error codes, atomicity, 25-op/25-key limits | P01 (exists: 13/24 agreed; fix the 11 diffs, e.g. `409 KEY_CONFLICT`, `400 CONDITIONAL_CHECK_FAILED`, `422 UNPROCESSABLE_ENTITY`, `KEY_DUPLICATION_ERROR`, `TOO_MANY_BATCH_ENTITIES`, `404 SCHEMA_NOT_FOUND`, `INCORRECT_PROPERTY_TYPE`) | the 24-case invocation, run twice | M | `s_webhook_replay`, `r_no_lost_update`, every storage path |
| KVS/CES query: default 10 / max 100, the 101 error, `beginsWith`, CES `where`/`filters`/`sort`, empty pages from `and`/`or`, cursor shape | P02 | seed N rows; walk queries | M | `r_continuation`, `r_migration`, `r_poison_items` |
| TTL: expiry granularity, expired-but-readable, `returnMetadataFields: EXPIRE_TIME`, TTL in the 4th argument of `transact().set` | P03 | TTL 60 s; read at +30/+65/+120 s and +10 min | M (readability window observed; the 48 h bound is D) | `s_webhook_replay`, `s_verdict_ttl` |
| optimistic concurrency: conditional transactions under parallel consumers; `FAIL_IF_EXISTS` on entity `set`; a condition on a missing entity (machinery N5) | P04 | push 50 events; each transacts on one key with a version condition; count conflicts | M | `r_no_lost_update` (+ the racy mutant must also lose updates LIVE, §I.4) |
| secret store round trip; secrets invisible to `query` | P05 | `setSecret`, `getSecret`, `query` | M | `s_secrets` |
| async events: redelivery schedule after throw and timeout, `retryReason` values (the 1.0 emulator's `FUNCTION_ERROR` is suspect vs `FUNCTION_TIME_OUT`, fidelity §3.2), `retryContext`, `retryAfter` clamp 900, 4 KB `retryData`, concurrency-key overlap, duplicates and ordering as observed, `delayInSeconds`, push limits (51 events, 201 KB), 100 KB per event for long consumers, the cyclic limit, `getStats`/`cancel` | P06 | the 1.0 plan of fidelity §3.2, rows 4–8 (≈ 40 min, mostly idle) | M (schedule, codes, limits); D (24 h retention, ordering as "no guarantee") | `r_kill_safety`, `r_poison_items`, `r_continuation`, `t_pool_wall_pause` |
| invocation timeouts: consumer 55 s default and `timeoutSeconds`; scheduled; webtrigger 55 s; resolver 25 s; partial writes persist after a kill; `invocationRemainingTimeInMillis` | P07 | functions that write a KVS marker every second until killed | M | `r_continuation`, `r_kill_safety`, `g_placement` |
| warm reuse within and across installations | P08 | a module-scope counter returned per invocation; two installations (Jira and Confluence products on wolfaenpak are separate installations) | M if observed; D otherwise ("may", rob#11) | `s_tenant_isolation` (graded only on crossings the emulator's stated reuse creates) |
| unawaited work resumes later or never | P09 | `setTimeout` after return writes a marker; observe whether and when it lands | M if observed, else D (rob#12 quote) | `s_tenant_isolation` |
| web trigger request and response: raw `body` string, header name case and arrays, `queryParameters`, static `outputs` mapping, `X-Ratelimit-*` on responses, `webTrigger.getUrl` URL form | P10 (partly done: request fields, 424 after uninstall) | signed and unsigned curls with mixed-case headers | M | `s_webhook_*`, `u_admin_secret` (URL display) |
| product events: `updated` with status, priority and move items; `deleted:issue`; a project delete emits no per-issue deletes; `selfGenerated` with and without `ignoreSelf`; `installed`/`upgraded` payloads; observed delivery delay | P11 | perform the actions on wolfaenpak (create, transition, move, delete issue and project); log payloads | M (shapes); D (3-minute bound, rob#33) | `r_sla_clocks`, `r_world_changes`, `r_self_events`, `r_migration` |
| scheduled triggers: first tick ≈ 5 min after deploy; `{statusCode:204}`; a throw is not retried | P12 | log ticks for 2 h | M (first tick, throw); D (duplicates, rob#40) | `r_scheduled_semantics` |
| Jira REST behaviours the app's protocols rest on: comment `properties` on create, read back through `POST /comment/list?expand=properties`; issue create with properties and labels; `search/jql` freshness after create with and without `reconcileIssues` (present in the shipped OpenAPI as a GET parameter and in `SearchAndReconcileRequestBean`); GET by an old key after a move; 404 after delete; `permissions/check` incl. `globalPermissions: ADMINISTER`; `mypermissions`; asApp vs asUser on a security-level issue, a reporter-only project and a ROLE-RESTRICTED COMMENT; rate-limit headers on ordinary responses (passive, no 429 provoked) | P13 | REST calls from the probe consumer (as app) and resolver (as user); Jira writes are permitted on wolfaenpak | M | `r_note_exactly_once`, `r_issue_exactly_once`, `r_world_changes`, `s_person_visibility`, `s_admin_authz`, `t_*` header grammar |
| the Custom UI bridge on real Jira: served form (bridge + resizer synchronous), `requestJira` forwarding rate-limit headers (FRGE-1923), `invoke` `rateLimitProperties` field names on success, `getContext` shapes for globalPage and issuePanel, `router.open`/`navigate`, `theme.enable` | P14 (browser, quiet window) | forge-live-harness profile | M | B rows, `u_board_table`, `b_boot_429` (field names) |
| Realtime: `publish` vs `publishGlobal` from resolver and consumer; `subscribe` vs `subscribeGlobal` pairing; claims filtering; subscribe-only and publish-only tokens; the context-scoped default; `errors[]` without a throw | P15 (browser, ≈ 1 h) | two browser sessions, two users | M | `u_board_live`, `s_realtime_isolation`, `u_panel_draft_flow` (1.0 defect D cannot recur: contract text = probe result) |
| the UI Kit host: callback arguments (Form `onSubmit`, Tabs `onChange`, UserPicker `onChange`, Textfield `number`/`password` events, Checkbox), DynamicTable sort by `key` on decimal and date fixtures, `testId`s delivered, ForgeDoc captured through an app-side `callBridge` spy | P16 (browser on real Jira) | the probe admin page logs every callback argument and reconcile payload | M (callbacks, sort); D (component semantics from `@forge/react` 12.3.0 source) | `u_admin_*`, `b_admin_boot` |
| Forge LLM: validation (`temperature`+`top_p`; parameters on opus-class), `list()` statuses, streamed tool-call chunking, `finish_reason` values, `usage`, the unknown-model error | P17 (cents of tokens; confirm the `llm` module is enabled for the space first) | invalid and valid calls; one streamed tool call | M (validation, list, stream shape); W (refusal and moderation shapes, stated) | `g_model_lifecycle`, `g_failure_states`, `g_forced_tool_validation` |
| JQL function: called on search; precomputation reused (no second call); the same fragment for two users; the computation update API changes results; > 1,000 values (the exact error); a 26 s handler (timeout, 1-minute error cache) | P18 (+ P18b: an indexed entity property usable in JQL, if the module is verified) | searches as two users on wolfaenpak | M | `s_jql_user_agnostic`, `r_sla_clocks` (breach set), the §C.10 scale path |
| lint and deploy: the server-rule corpus extended to every 2.0 manifest feature, including two `jira:globalPage` modules, the UI Kit resource as a directory, the static-trigger outputs, consumer `function`/`resolver` forms, `timeoutSeconds` placement, `memoryMB`, index name and range rules, > 5 scheduled triggers or 2 `fiveMinute`, `unsafe-*`, runtimes; deploy-test v04–v06; `forge eligibility` on the golden | P19 (≈ 6.5 s per manifest through the real check; ≈ 40 manifests ≈ 5 min) | the real `forge lint` and `forge deploy --no-verify` | M | `l_deployable`, `l_roa_eligible` |
| Rovo action context: `accountId` from context, input coercion | P20 (only if Rovo is enabled on wolfaenpak) | one action call | M if available; else D (sec#26) | `s_rovo_inputs`, `s_identity_from_context` |
| the install race: 401/403 on early asApp calls | P21 | install, then call asApp at once | M if reproduced; else D (rob#37 quote; CHANGE-3445 is rolling out) with the stated window | `r_install_race` (.005) |
| bridge injection into named Custom UI entries | P-ENTRY | one resource with two entries | M | none until measured (multi-entry is allowed, never required) |

### I.3 Lint fidelity (L0)

`npm run lint` in the kit is built from four parts.
1. The CLI's client half, unchanged (`@forge/lint` 6.3.0, `@forge/manifest` 13.6.0, `@forge/cli-shared` 9.7.0,
   byte-identical to CLI 14.1.0).
2. The **measured server-rule pack** (fidelity §2.5). Each rule is a predicate over the interpolated manifest with its
   wolfaenpak receipt: CLI version, requestId and exact message. Six rules exist today: v01/02/03/07/08/10.
3. The pinned deprecated-runtimes flag value.
4. A CLI-equal file walk (`.mjs`/`.cjs` included, UI Kit resource directories included, `.gitignore` honoured).

Only measured rules can fail `l_deployable`. Docs-only constraints (fidelity v04–v06) are reported and never charged.
The drift job is run online by us, never by entrants. It re-runs the corpus through the real CLI, re-fetches the flag
value and the OpenAPI files, and compares the newest `@forge/cli` dependencies with the kit lock; a drift blocks the
next release until the pack is re-frozen.

### I.4 Real Forge as a gate, never a score input (L2/L3)

- **Golden gate at freeze.** The golden deploys and installs on wolfaenpak and passes a scripted smoke test through
  forge-live-harness:
  - a signed CRM POST becomes an escalation and an issue;
  - a note becomes a comment;
  - the panel and the board load with data;
  - the admin page loads and saves a policy;
  - `escalationBreached()` answers in a JQL search;
  - `forge eligibility` reports RoA-eligible.

  Then uninstall, and delete the created issues.
- **Platform-semantics mutants must misbehave live too.** `m_rmw_unconditional` under P04's parallel burst must lose
  updates on real Forge. `m_nonce_get_then_set` must double-apply on a concurrent live pair. Otherwise the mutant
  measures the emulator, not Forge (fidelity §4 L2a).
- **Audit lane for our own baseline runs** (fidelity §4 L2b). Each published baseline's final tree is deployed, with
  no install, to a dedicated audit app. Accept or refuse, with its messages, is compared to the scorer's L verdict; a
  mismatch blocks publication until the rule pack is fixed and the build is rescored.
- **Browser calibrations (L3),** in quiet windows only. P14, P15 and P16, plus one ranking check: the local boot lane
  must rank the golden ahead of `m_waterfall` and `m_dev_react` in the same order real Jira's time-to-READY does.

### I.5 What stays unmeasured, and how the design avoids grading it

| unmeasured | why it cannot be measured | how the design avoids a guess |
|---|---|---|
| Tier 1 pool behaviour, forgiveness, burst capacities, per-endpoint costs, other apps' traffic | "Do not perform rate limit testing against Atlassian cloud tenants" (tiers#48); unpublished | class W: `RATE-MODEL.json` owns every value and says it is the benchmark's own world. Only the header GRAMMAR rests on docs and passive observation (P13) |
| real latencies | variable, and not a platform contract | class W latency model, stated. Freshness objectives are set at ≥ 2× the golden's worst seed, so no row turns on a plausible latency difference |
| 24 h and 96 h retention, a 3-minute event delay bound, scheduled duplicates, skips and overlaps | too long or not forceable live | class D: doc quote + stated emulator choice. A correct app passes for every value inside the documented envelope |
| KVS `query` lag magnitude | "slightly out of date" only | class D with a stated lag. Graded only through outcomes that a get-based design passes for any lag |
| maximum concurrency keys, maximum `concurrency.limit` | undocumented (rob#19 open 1) | the emulator imposes no maximum and no row depends on one |
| LLM refusal, moderation and 429 shapes | not forceable at low cost | class W shapes stated; `g_failure_states` accepts any `ForgeLlmAPIError` status for "error" (1.0 precedent) |
| UI Kit product host internals (invalid xcss, unknown props, reconcile batching) | the renderer is closed (uikit#13) | allowlisted components; anything else renders a loud "unmodelled" marker; never pixels; only P16-measured callback shapes |
| CPU and memory timing (OOM, slow code) | load-sensitive | not graded. `memoryMB` is honoured as a heap cap only to keep runs bounded; no row tests OOM |
| constant-time comparison | not observable deterministically | not graded (§F.7) |

---------------------------------------------------------------------------------------------------------------------

## J. Build plan, effort, risks, size, budget, cost, scoring time, packaging

### J.1 Freeze gate (all must hold; `score_forge2.py --reference` enforces G1–G6)

| # | gate | why |
|---|---|---|
| G1 | **load-robustness:** golden, alt and every mutant scored twice, idle and with every core saturated by a synthetic CPU+IO hog → byte-identical verdict JSON apart from wall-time fields | the property 1.0 lacked (`--quiet-load`) |
| G2 | golden 1.000 on 3 scoring seeds + 20 calibration seeds; every critical exactly 1.0; no `unavailable`, no `harness_missing`; every contract hook produced non-vacuous evidence | 1.0 §13.4 item 1 |
| G3 | severity selftest (§F.10) | 1.0 §8.6 |
| G4 | the independent alt app (separate session, public text only, deliberately different choices — §H item 7) ≥ 0.95, zero `harness_missing` | proves the text suffices |
| G5 | each one-defect mutant loses exactly its declared rows (± ROOT_BLOCKS); racy mutants fail on 20/20 calibration seeds | a race that manifests by luck is not a check |
| G6 | empty starter and one-function app ≤ 0.05; "202-only intake" ≤ 0.10 | vacuity |
| G7 | conformance: every emulated behaviour's probe identical live vs emulated, or explicitly classed D/W in `FIDELITY.md` with its quote | §I |
| G8 | lint corpus: kit lint = the real CLI on every corpus manifest, including the golden's, the alt's and every mutant's | §I.3 |
| G9 | the real-Forge golden gate and the live platform-semantics mutants (§I.4) | the mutants measure Forge, not the emulator |
| G10 | fairness invariant: the dev world's event log carries every behaviour-class tag of §D.4 | §H item 4 |
| G11 | phase 4 pre-build proof done; every "could not know" resolved | NOW.md phase 4 |
| G12 | one entrant-path control run from the Benchmark view (gate 3) with a cheap model, before the paid Sol pilot | 1.0 §13.4 item 7 |

### J.2 Reference app size (estimate)

| part | files | LOC |
|---|---:|---:|
| intake (verify, nonce, payload, push) | 3 | 220–300 |
| apply consumer (CRM, issue events, notes, idempotent create and post, per-issue pacing) | 6 | 700–900 |
| SLA engine + policy | 2 | 300–400 |
| reconcile, sweep, continuation, leases | 4 | 450–600 |
| dosing client (cost table, sharded accounting, pool state, header parser, pacing, retry) | 4 | 400–550 |
| verdict cache, roles, admin authorisation | 3 | 250–350 |
| migration (1.x reader, dual read, watermark) | 2 | 250–350 |
| JQL function + precomputation sync | 2 | 150–220 |
| LLM worker, cache, budgets, Realtime tokens | 4 | 400–550 |
| resolvers + Rovo action + export | 4 | 450–600 |
| Custom UI board + panel (React, one entry each, shared lib) | 12–16 | 1,500–2,000 |
| UI Kit admin (7 tabs) | 8–10 | 900–1,200 |
| manifest, build scripts | 3 | 200–260 |
| **total** | **≈ 55–60** | **≈ 5,900–7,900** |

That is about 3–4× Forge 1.0's golden (958 src + 982 UI).

### J.3 Recommended call budget: 250 (the owner decides; alternative 200)

- **The estimate.** A disciplined frontier model needs ≈ 110–170 calls:
  - reading the contract, typings, schema, OpenAPI and `RATE-MODEL.json`: 20–30;
  - writing ≈ 6–8k LOC at Pareto's 1.0 rate of about one file per call plus larger multi-file calls: 45–65;
  - lint and manifest repair: 5–15;
  - `forge-dev world run` cycles, browser checks and fixes: 35–60.
- **The 1.0 evidence.** The frontier used 50–72 of 150 calls and stopped by itself (frontier-behaviour §3).
- **Why 250 and not less.** A budget that BINDS measures speed, not depth: *"A limit would mostly punish exploration
  style"* (frontier-behaviour §5.3). 250 gives 1.5–2.3× headroom over the estimate, so a strong model is limited by
  engineering, not by the counter. A weak model will exhaust 250 the way it exhausts 150.
- **Mechanism.** `bench_budget.CALL_BUDGET` is shared with the Gauntlet, so 2.0 needs a per-tier budget field
  (integration D5). The prompt states the number (the 1.0 parity test).
- **If the owner keeps 150,** cut in this order, decided now, never during a run:
  1. the weekly digest (part of `g_dosing` and `g_economy`);
  2. the Rovo action (`s_rovo_inputs`; the auditor path stays `admin.export`);
  3. the JQL function (`s_jql_user_agnostic`, §C.10);
  4. brownfield (`r_migration`, C4's 1.x part).

  The pipeline, security, dosing, boot, admin and LLM draft stay.

### J.4 Per-run model cost (estimates; the pilot replaces them with a measurement)

The anchor is Forge 1.0: Sol cost $0.72 in 13 min. That implies roughly 3M prompt tokens at ≥ 95% cache reads and
≈ 15k output tokens; Pareto used 3.2M prompt and 31k output on 50 calls.

For 2.0, assume ≈ 2.5× the calls, ≈ 2× the average context, ≈ 6× the output, and cache reads ≥ 95% (an assumption).

| model | prices (in / out / cache-read, $ per M) | prompt tokens | output | per run |
|---|---|---:|---:|---:|
| GPT-6.1 Sol | 2 / 10 / 0.10 | 9–26 M | 60–150 k | **$2.3–6.6, central ≈ $3.8** |
| Opus 5.5 | 4 / 20 / 0.20 (NOW.md) | same profile | same | **$4.6–13, central ≈ $7.6** |
| cheap (GPT-6 Luna, 1.0 §11 prices 0.10 / 0.50 / 0.01) | 0.10 / 0.50 / 0.01 | ≈ 30 M (budget exhausted) | ≈ 300 k | **≈ $0.35–0.9** |

A broken provider cache multiplies the input side 10–20× (1.0 §11). Arm `BENCH_MAX_USD` at $25 for Sol and Opus runs.
Scoring costs no model calls.

### J.5 Scoring time (serial, one host, three scoring seeds)

| step | time |
|---|---|
| lint ×2 (client + server pack), bundle, warm-load every function | ≈ 25 s |
| per seed: the lockstep backend over 6 virtual hours (≈ 4–6k invocations, ≈ 40–70k proxied calls at ≈ 0.5–1 ms each incl. IPC; virtual waits cost nothing) | ≈ 40–80 s |
| per seed: the Node UI Kit lane (CP1, CP4) | ≈ 10–15 s |
| per seed: the browser lane (CP2–CP4; ≈ 10 cold loads incl. the 429 boot and the N/4N scaling page, draft flow, live update, XSS, dark mode) | ≈ 45–75 s |
| per seed: indexed oracle + composition | ≈ 5 s |
| **total** | **≈ 6–10 min** (1.0: 6–7 min; two pathological 1.0 apps took ≈ 20 min, and 2.0 bounds such apps by the stated concurrency, never by a cap on their grading) |

Every row is a function of virtual time and counts, so scoring needs no quiet machine. Gate G1 is the proof.

### J.6 Packaging added

| item | size | note |
|---|---|---|
| desktop payload (the `forge2/` tree: public, starter, kit code, site, world generator; `score_forge2.py`, the probe, oracle, hosts) | +3–4 MB | own tree and modules (integration D1) |
| OpenAPI copies | +0 if shared with 1.0's payload by path, +7.1 MB if duplicated | |
| kit cache, one `npm ci` per kit version, outside the sandbox | +≈ 145 MB for the `@forge/react` 12.3.0 closure (282 packages, 143.6 MB by registry metadata, ≤ 306 MB on disk) | run one real `npm ci` + `du` before freezing (BRIEF §5.0); some `@atlaskit` overlap with the bridge already in the 1.0 kit |
| SQL engine, Confluence mock, container runtime | **0** | excluded by design (§0.3) |
| Chromium / Playwright | 0 new | already required by 1.0's probe |
| evidence per run | ≈ 20–60 MB | page summaries instead of full bodies (machinery N9), NDJSON spill; the clip ≤ 4 MiB (site limit) |

### J.7 Work packages (one owner per file; agent-days describe blast radius, not priority)

| WP | owns | agent-days | depends on |
|---|---|---:|---|
| WP0 fidelity probes P01–P21 + conformance harness + `FIDELITY.md` | `forge2/probe/`, fixtures | 3–4 (+ ≈ 5 h of mostly idle live wall) | — |
| WP1 lockstep core: warm workers, virtual-time agent, scheduler, proxy choke point, kill-by-pid, unawaited freeze, watchdog, determinism tests | `forge2/kit/lib/{runtime,runner,scheduler,proxy}` | 6–8 | — |
| WP2 mock Jira 2.0 + points gateway + scale generator | `forge2/site/**` | 9–12 | WP0 (P11, P13, P18) |
| WP3 platform emulation: async, scheduled, web trigger, KVS/CES aligned, secrets, lifecycle, Realtime, LLM responder, Rovo | `forge2/kit/lib/{emulator,kvs,realtime,llm,webtrigger}` | 6–8 | WP0, WP1 |
| WP4 UI Kit Node host | `forge2/kit/lib/uikit-host/**` | 4–6 | WP0 (P16) |
| WP5 browser lane: waves, CDP, CSP, chrome outside the frame | `forge2/kit/lib/bridge-*`, `bench/forge2_probe.mjs` (browser part) | 3–4 | WP1 |
| WP6 scorer, oracle, composition, selftest, calibration | `bench/score_forge2.py`, `bench/forge2_oracle.py` | 8–10 | WP2, WP3 |
| WP7 golden (5–6), alt (4–5, separate session, never sees the golden), ≈ 80 mutants (4–5) | `bench/golden-forge2/`, `bench/golden-forge2-alt/`, `forge2/mutants/` | 13–16 | WP8 public text |
| WP8 public text, `RATE-MODEL.json`, `CONTRACT-SCHEMAS.json`, dev tools, lint pack | `forge2/public/`, `forge2/kit/bin/` | 4–5 | WP0 |
| WP9 integration: FORGE20 tier, per-version release manifest, per-tier budget, desktop per-era tables, site per-era snapshots and validators, catalog proxy | integration.md §3–§7 | 5–7 | WP6 |

Total ≈ 61–80 agent-days: about 3–4 calendar weeks at ~3 concurrent agents. **Order:** WP0 and WP1 first and in
parallel, because fidelity decides the emulator and the core decides everything. Then WP2–WP5 and WP8; then the
golden against the dev tools; WP6; the alt and mutants; the gate; the cheap control run; the Sol pilot.

### J.8 Risks, ranked by confidence that the design holds as written (not by effort)

| # | risk | confidence | mitigation |
|---|---|---|---|
| R1 | **The lockstep core with Atlassian's wrapper.** Warm multi-invocation serving plus virtualised timers (undici and the wrapper's own timers) may not be deterministic at the first attempt (machinery §4.2 rates deterministic interleaving MEDIUM-LOW) | **MEDIUM-LOW** | WP1's first task is a spike: the pinned wrapper serving 2 invocations in one warm worker under the agent, plus a double run under a CPU hog. If warm serving is impossible, fall back to fresh processes per invocation with seeded module-state SNAPSHOT replay; `s_tenant_isolation` is then graded on stored and written crossings only, and the contract says so |
| R2 | calibrating budgets, objectives and boot numbers so the golden passes with ≥ 2× margin while naive mutants fail | MEDIUM | 20 calibration seeds on the golden and the alt; numbers frozen into the contract; any number a model must compute against is stated |
| R3 | UI Kit host fidelity (the closed product renderer) | MEDIUM | P16 fixtures; semantics only; allowlist + loud "unmodelled" |
| R4 | mock Jira breadth (security levels, role-restricted comments, moves, labels, custom-function substitution, search freshness) | MEDIUM | the OpenAPI-driven 501 → HELD verdict (1.0 R5); the alt app's different endpoints prove coverage |
| R5 | Realtime isolation semantics (P15) may differ from the docs (1.0 defect D's history) | MEDIUM | contract text = the probe result verbatim; `s_realtime_isolation` is re-specified if tokens do not enforce claims as documented |
| R6 | worker memory at the stated concurrency (≈ 40 workers × 60–80 MB) | MEDIUM-HIGH | `worker_threads` isolates as an optimisation, or lower stated concurrency |
| R7 | too hard: the frontier < 0.30 | LOW-MEDIUM | the phase 4 pre-build proof; the J.3 cut list decided in advance |
| R8 | too easy: Sol ≥ 0.85 with no critical | LOW-MEDIUM | harden with the G.5 levers before any other run |
| R9 | ≈ 27 KB prose invites a desk audit | LOW-MEDIUM | tables moved to JSON; the K.2 trim option |
| R10 | supply and licence: wrapper CDN asset, OpenAPI files, `@forge/*` redistribution (1.0 R4, uikit open 8) | MEDIUM | sha-pinned caches; install from the registry at kit time; owner decision K.11 |

---------------------------------------------------------------------------------------------------------------------

## K. Open decisions for the owner

1. **Call budget:** 250 (recommended, §J.3), 200, or 150 with the pre-decided cuts.
2. **Public input size:** accept ≈ 27 KB of prose + ≈ 6 KB of tables, or trim to ≤ 25 KB by moving §E.10's fact lines
   into a `PLATFORM-FACTS.md` (still public input, read on demand).
3. **Brownfield:** keep the 1.x migration (recommended: the class the frontier has never been tested on), or cut it.
4. **The JQL scale path (> 1,000 matches):** keep it (recommended), pending P18. If real Jira behaves differently
   than documented above 1,000, re-scope before the contract freezes. Verify the `jira:entityProperty` module's status
   (not in the verified brief) before the mock models `issue.property[...]`.
5. **Critical stacking:** five criticals multiplying with no floor (1.0 parity), or a product floor of 0.36 (at most
   two count).
6. **The stated world numbers:** 12,000 points per installation-hour; 25% per 5 minutes; 4 in flight; a 10-minute
   verdict TTL; 10/30/10-minute freshness objectives; the boot budgets; 5 drafts per person-minute. Approve them after
   calibration; they become contract text.
7. **The Rovo action:** keep it (a security surface plus a second read path), or drop it.
8. **Scoring seeds:** three (≈ 6–10 min, worst per row; recommended) or two.
9. **Forge 1.0's fate** (fix and re-score, or freeze with a note): it must be settled before the 2.0 integration
   (integration §7 step 0).
10. **Pilot:** Sol alone as the acceptance test (NOW.md), or Sol and Opus before the full rerun.
11. **Platform facts source:** our one-line facts with doc URLs inside the contract (recommended), or verbatim
    Atlassian doc excerpts shipped offline (a licence question).
12. **Tier letters** L S R T B U G replace 1.0's set. The desktop and the site need per-era tier tables
    (integration D3); confirm the letters before WP9.

