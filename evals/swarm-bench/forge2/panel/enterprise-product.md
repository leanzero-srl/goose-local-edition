# Forge 2.0 design — ENTERPRISE PRODUCT angle: **Changeproof**

*Change assurance and segregation-of-duties evidence for regulated Jira.* Panel design, 2026-10-09. Angle: the most
realistic product a large Atlassian customer would pay for, which needs every mandate item for its own reasons.

Inputs read whole: `forge2/NOW.md`, `research/BRIEF.md`, `research/{tiers,security,robustness,llm,uikit,bootspeed,
platform-2026-10-09}.md` (the sections cited below), `research/understand/{frontier-behaviour,real-forge-fidelity,
machinery,integration}.md`, `forge/DESIGN.md` §2, §3, §8, §13.4–13.5, §17, `research/forge1-review/*.md`, and the
1.0 public text. Nothing was run. Every platform fact below is a BRIEF `confirmed` claim (tags such as `tiers#1` point at
the topic notes' verification tables). Every number marked *proposal* is a benchmark value that calibration on the
golden and the alt app freezes into the contract.

---------------------------------------------------------------------------------------------------------------------

## 0. One-screen summary

| | |
|---|---|
| Product | **Changeproof.** A Forge app for banks, insurers and utilities. It keeps a tamper-evident evidence chain for every production change: approvals from Jira, deployments attested by CI/CD over a signed webhook, and implementations and material edits from Jira history. It computes segregation-of-duties (SoD) verdicts and release posture, stamps evidence onto issues, drafts auditor narratives with Forge LLM, and gives admins a native UI Kit console. |
| Why this vehicle | Each mandate item is something a regulated customer's auditor, CISO, procurement or platform team demands (§A.3). Nothing is added for the benchmark's sake. |
| Shape | 3 installations on ONE Tier 1 pool (two are upgrades from v1 that hold v1 data, one installs fresh mid-run) · 2 Custom UI front surfaces (`jira:issuePanel`, `jira:globalPage`) · UI Kit 2 `jira:adminPage` · static HMAC `webtrigger` · issue and lifecycle triggers · 9 queues (including v1's `cp-events`) · 2 scheduled triggers · Forge LLM · Realtime · KVS + Custom Entity Store + secret store · Rovo action ×2, agent, `rovo:mcp` · Privacy API. Runs on Atlassian (RoA) eligibility is required. |
| Difficulty levers (all STATED) | concurrency with a throughput bar on a fork-free hash chain · backfill at scale under a per-installation Tier 1 budget, with smoothing and completion bars · a quota wall caused by the app's other customers · ambiguous write failures with exactly-once stamps under per-issue write limits · live migration of v1 data · world changes mid-run · permission-cache TTL against points · LLM grounding, injection, dosing and per-visibility caching · count-based boot budgets on an issue panel that loads on every issue view. |
| Public text | contract ≈ 24 KB + tables ≈ 5 KB + prompt ≈ 3 KB + starter ≈ 4.5 KB = **≈ 36 KB**. A platform reference pack (≈ 70 KB of verbatim Atlassian doc excerpts) is consulted on demand, like the typings. |
| Grading | 101 weighted rows in 12 tiers, plus 6 diagnostic rows and 3 excellence rows. 10 criticals, each firing only on its named consequence, observed. 4 admission bands. Two scoring worlds; the concurrency phase is replayed under 3 deterministic schedules; the worst result counts. Everything is measured in counts, state and VIRTUAL time. |
| Predicted | golden 1.000 · alt ≥ 0.95 · **GPT-6.1 Sol 0.45–0.75** (central ≈ 0.65) · Opus 5.5 0.45–0.78 · Haiku 5.5 0.30–0.55 · weak models 0.00–0.30. Confidence MEDIUM-LOW, because Sol's and Opus's 1.0 trees are not on this machine. |
| Budget / cost / time | recommend **300 calls** as a per-tier field · Sol **$5–12** per run · cheap model ≈ **$1** · scoring **20–35 min** (estimate) · reference app ≈ **9,000 LOC** · kit cache **+130–280 MB** (the UI Kit closure) · desktop payload **+≈ 3 MB**. |

---------------------------------------------------------------------------------------------------------------------

## A. Product pitch

### A.1 The product

**Changeproof** answers one question that every regulated software organisation is asked each quarter: *"Show me
that every change that reached production was approved, by someone other than the person who built it and the person
who deployed it, before it was deployed, and that nothing material changed after the approval."* Today that answer is
assembled by hand from Jira history, CI logs and spreadsheets.

Changeproof is installed on the customer's Jira Cloud site:

- **It ingests evidence from two systems.** Jira supplies approvals (a Change-approval select field), implementations
  (transitions into a done status), material edits (fields the policy names, such as fix version and description) and
  revocations. CI/CD supplies deployments, posted to a signed web trigger by Jenkins, GitHub Actions or Bamboo.
- **It keeps a per-change evidence chain.** It is append-only and hash-linked, so an export can be verified offline by
  the external auditor.
- **It computes a verdict per change** (`compliant`, `violation` with reason codes, `no-deployment`) **and a posture
  per release** (counts and gate-ready).
- **It shows the evidence where people work.** An **issue panel** shows "Change evidence" on every issue. A **global
  Assurance workspace** gives release managers and auditors posture, drill-down, export and AI-drafted control
  narratives.
- **It stamps evidence onto the issue.** Each production deployment gets one ADF comment, so the evidence is visible in
  Jira's own activity stream.
- **It has a native admin console** (UI Kit 2, in Jira administration): policy, regulated projects, roles (compliance
  admin, auditor), the CI signing secret, budgets and usage, the failed-work queue, and migration progress.
- **It gives auditors' AI assistants** read access through Rovo actions and MCP.

### A.2 Who pays, and why it is not a tutorial

The buyers are regulated enterprises: banks under DORA's ICT change-management requirements, SOX-listed companies with
IT general controls over change management, and ISO 27001 organisations. These customers already run Jira at scale and
buy governance apps from the Marketplace. The product is not benchmaxxed:

- Its semantics (SoD across three actors, evidence invalidation by material edits, posture per release) are rare in
  training data.
- Its scale is the normal scale of an enterprise Jira site.
- Every hard requirement below is a line item from a real procurement questionnaire or an audit finding, not a puzzle.

### A.3 Customer demand → stated requirement (the credibility table)

| who demands it (persona) | demand, in their words | becomes requirement | mandate item |
|---|---|---|---|
| External auditor (Big Four) | "Evidence that can be edited is not evidence." | hash-chained, append-only, exportable, verifiable chain; fork-free under concurrency | 3, 5, 11 |
| Head of Internal Audit | "Approver ≠ implementer ≠ deployer, approval before deploy, re-approve after material change." | the §1 verdict rules, exact | 3 |
| Release manager | "A release train deploys 400 services in four minutes; I need posture within two minutes, not after lunch." | freshness SLO under burst (120 virtual s) | 5, 9 |
| CISO | "No unauthenticated endpoint touches our audit trail." | app-implemented HMAC + timestamp window + replay protection on the web trigger; no side effect before verification | 4 |
| CISO | "Admin functions only for admins, enforced server-side; secrets never leave the vault." | resolver-side role checks; secret store; presence-only read-back | 4, 8 |
| Data protection officer | "Departed employees' names must be erased." | Atlassian Personal data reporting with erase and refresh | 4, 9 |
| Procurement | "Runs on Atlassian badge required: data residency, no egress." | RoA-eligible manifest; static web trigger; no remotes or external domains | 1, 4 |
| Atlassian platform team (the vendor's own SRE) | "Our app shares ONE 65,000-point hourly pool across all customers. One customer's backfill must never take every other customer down." | per-installation background budget, smoothing, near-limit pause, quota-wall pause, interactive priority | 9 |
| Jira admin at the customer | "The panel loads on every issue view for 3,000 users. It must not slow Jira." | count-based boot budgets on the issue panel; no LLM call and no write at boot | 6, 7 |
| Model-risk officer | "AI drafts are labelled, cite real evidence, never assert compliance the data does not show, and are published only by a human." | grounding, citations, injection resistance, confirm-to-publish, budgets | 10 |
| Compliance admin | "Configure it in Jira admin, natively." | UI Kit 2 admin page with forms, tables and validation | 8 |
| Existing v1 customers | "Upgrading to v2 must not lose one record." | live v1→v2 migration, exactly once, while events flow | 5 |

---------------------------------------------------------------------------------------------------------------------

## B. Modules

Statuses are from BRIEF §1.1 (`[B]` = verified live on 2026-10-09).

| # | module | status | role in Changeproof | why a customer needs it | what makes it hard |
|---|---|---|---|---|---|
| 1 | `jira:issuePanel`, Custom UI | GA [B] | "Change evidence": verdict, reasons, chain records, chain-verified mark | evidence where engineers and approvers work | Boot on EVERY issue view (count budgets, §C.6). One server-side permission check per view costs points, so the stated TTL cache must balance security against Tier 1. IDOR through payload or extension ids. XSS through CI pipeline names. |
| 2 | `jira:globalPage`, Custom UI | GA, one per app [B] | "Assurance workspace": releases, posture, change drill-down, export, narratives | release managers and auditors | Router deep links. App-level paging cursors stable across invocations. Live posture over Realtime with no polling. Per-person export throttle (button-masher). The auditor scope rule. |
| 3 | `jira:adminPage`, `render: native`, `useAsConfig` | GA (uikit#18, #20) | UI Kit 2 admin console | native configuration | Display conditions run client-side, so authorization must be in the resolvers. UI Kit host semantics: `onSubmit` gets no data; UserPicker stores an object; Textfield `email` stores the event; sort keys. No subpages for UI Kit (use Tabs or Router). |
| 4 | `trigger` (`avi:jira:created:issue`, `avi:jira:updated:issue`, `avi:jira:deleted:issue`, `avi:forge:installed:app`, `avi:forge:upgraded:app`) | GA | ingest Jira facts and lifecycle | real-time evidence | At least once, unordered, up to 3 min late (rob#33). Self-generated events. Install event before permissions (rob#37). At most 4 app-requested retries on product events (rob#34). Only top-level deletes (rob#36). |
| 5 | `consumer` ×9 queues | GA, `@forge/events` 3.0.7 (v2+ semantics) | facts, attestations, stamps, sweep (reconcile), backfill, migration, llm, privacy, and v1's `cp-events` | decoupling and throughput | Concurrency keys are per installation across queues (rob#28). Retry by RETURNING `InvocationError`, `retryAfter` ≤ 900 (rob#24). 24 h retention (rob#21). 100 KB events for long consumers (rob#16). 500 events/min, 50 per push, cyclic 1,000 (rob#15, #18). `jobId` is not an idempotency key (rob#20). Lowest timeout wins (rob#3). |
| 6 | `scheduledTrigger` ×2 (`hour` reconcile, `day` privacy) | GA (≤ 5, ≤ 1 fiveMinute) | heal and incremental sync; personal-data cycle | reconciliation, not trust in the stream | Duplicates (rob#40). A throw is not retried (rob#39). Distributed per installation (tiers#39). Return `{statusCode: 204}` (rob#41). |
| 7 | `webtrigger`, `response.type: static` with outputs | GA; only static triggers are RoA-eligible (sec#18) | CI/CD deployment attestations | CI integration that survives an audit | No platform authentication by default (sec#15). App HMAC over the RAW body. Headers are arrays and case-insensitive. Atomic nonce. Acknowledge within 10 s or the sender retries. Bursts. |
| 8 | `llm` (`@forge/llm` 1.0.7) | GA since 2026-07-29/30 (llm#1) | control-narrative drafts per release | auditor productivity | No structured outputs, so a forced tool plus validation (llm#3, #10). `list()` lifecycle. Sampling rules (llm#16, #17). 100 RPM/installation and 500k TPM/model (llm#20, #21). No free allowance (llm#29). Placement: 25 s resolvers against 5-min consumers (llm#23). Injection. Permission-filtered prompts. |
| 9 | Realtime (`@forge/realtime` 1.0.1, bridge `realtime`) | GA (CHANGE-3326) | live posture; narrative delivery to the requester | live operations room | Claims derived server-side (Atlassian's own tutorial is exploitable, llm newer-fact 5). Publish-only and subscribe-only tokens. 50 ops/s including subscribes (rob#63). `publish` returns `errors[]` and does not throw. Pairing rules ([B] §11.1 D). |
| 10 | KVS + Custom Entity Store + secret store (`@forge/kvs` 2.0.7) | GA | chains, verdicts, governor ledger, roles, config, signing secret | the audit trail itself | Conditional CES transactions. `FAIL_IF_EXISTS` → 409 `KEY_CONFLICT` (measured). Query is eventually consistent. Cursors are not persistable (rob#57). 10 KB units (rob#52). 25-operation limits. `transact().set` TTL position (rob#50). TTL lag of 48 h (rob#47). One range attribute per index (server lint). |
| 11 | Rovo `action` ×2 (`get-change-evidence` GET, `open-review` CREATE), `rovo:agent`, `rovo:mcp` | GA / GA / Preview (CHANGE-3495) | auditors' AI assistants, inside Rovo and through MCP clients | 2026 audit practice | Inputs are untrusted except context (req. 13, sec#25, #26). `actionVerb` must be truthful (req. 14). Admin-level actions need the app's own checks. |
| 12 | Personal data reporting (`privacy.reportPersonalData` or `POST /app/report-accounts/`) | documented (sec#30) | erase and refresh stored names | GDPR | ≤ 90 accounts per request. The helper fires every batch concurrently (sec#31). 429 with Retry-After. |
| 13 | Bridge `invoke(…, {rateLimitProperties: true})`, bridge `requestJira` headers | GA (CHANGE-3314; FRGE-1923 fixed 2026-04-09) | 429-aware boot; exempt user-context reads | | |

**Considered and excluded (owner may revive, §K):**

| module | reason |
|---|---|
| Forge SQL (GA) | Needs a MySQL-compatible engine in the kit. That is a packaging and fidelity cost, and the RFC-148 images are unverified. |
| Confluence | A second mock product (machinery §4.1 MEDIUM). Mostly volume per frontier-behaviour §5.3. |
| `apiRoute` | Preview; the request shape is undocumented. |
| `global:fullPage` | Preview; the GA `jira:globalPage` serves the workspace. |
| `dashboards:widget` | GA, but 1.0 already covered it and it adds volume, not difficulty. |
| `rovo:skill` | Preview; frontmatter rules are wiring that 1.0 measured as solved. |
| app-managed permissions | Preview; `appContext.permissions` filling is unmeasured (machinery §4.7 HIGH risk). Kept as an optional extension. |
| `jira:jqlFunction` | GA and strongly emulatable; the best candidate if the owner wants one more module (§K). |
| Object Store, workflow modules, `jira:command`, `global:ui`, `dashboard:filters`, Containers, agentConnector | Preview, EAP or non-offline. |

**Multi-entry Custom UI resources** (GA, CHANGE-3337) are ALLOWED but not demanded. BRIEF §1.1 flags that bridge
injection into named entries was never observed. The fidelity probe I-21 decides whether the contract may encourage
them.

---------------------------------------------------------------------------------------------------------------------

## C. Architecture

The contract states OUTCOMES. The storage design, topology and dosing mechanism below are the GOLDEN's design: one way to
meet the contract, shown to prove feasibility and size the work. They are not requirements. The grader reads state only
through stated interfaces (§C.3 `audit.*`, the surfaces, the Rovo action) and through platform logs (the gateway,
storage, queue, LLM, Realtime, egress and console logs).

### C.1 Data model

**The evidence model (stated in contract §1; this is graded semantics).**

- **Fact.** A typed, immutable event about one issue: `approval`, `revocation`, `implementation`, `material-change`,
  `deployment`, `deletion`.
  - Identity is (`kind`, `ref`, `issueId`). `ref` is the Jira changelog id for Jira facts, the attestation's
    `deploymentId` for deployments, and `v1:<n>` only for v1-only facts.
  - Attributes are `at` (UTC, `YYYY-MM-DDTHH:mm:ss.sssZ`), `actorId` (accountId, or `null` for pipelines), and `env`
    for deployments.
- **Chain.** Per issue, the facts in append order: `seq` = 1..n, `prev(1)` = 64 × `0`, and
  `hash = sha256_hex(prev ⏎ issueId ⏎ seq ⏎ kind ⏎ ref ⏎ at ⏎ actorId ⏎ env)`.
  - The hash covers ids only, never display names, so a privacy erasure never breaks a chain.
  - Chains are append-only. Facts are appended in the order the app learns them, so `at` need not be monotonic.
- **Verdict** (evaluated with the policy in force).
  - An approval is *valid at t* when no revocation and no material-change fact of the same issue lies in (approval.at, t).
  - The implementer at t is the actor of the latest `implementation` before t.
  - A production deployment D is *compliant* when a valid approval A exists at D.at with A.actor ≠ implementer, and
    A.actor ≠ D.actor when D.actor is a person.
  - The verdict of a change is one of:
    - `no-deployment` when it has no production deployment;
    - `compliant` when every production deployment is compliant;
    - otherwise `violation` with the reason set ⊆ {`self-approved`, `approved-by-deployer`, `unapproved`,
      `approval-revoked`, `changed-after-approval`}.
- **Posture** of a release (current fix version): counts by verdict over its non-deleted changes, and
  `gateReady` = no violation and every change holds a valid approval now.

**Golden storage (one valid design; ≤ 20 entities, ≤ 7 indexes each, exactly one `range` attribute per index).**

| store | key / attributes | indexes | purpose |
|---|---|---|---|
| CES `evidence` | `issueId` s, `seq` i, `kind` s, `ref` s, `at` s, `actorId` s, `env` s, `prev` s, `hash` s, `source` s, `actorName` s | `by-issue` (partition `issueId`, range `seq`) | the chain |
| CES `chainhead` | key = `issueId`; `seq` i, `hash` s | — | append serialization by CONDITIONAL transaction (`seq == expected`); created with entity `set` + `FAIL_IF_EXISTS` |
| KVS `fact:<sha1(kind,ref,issueId)>` | `{seq}` | — | exactly-once marker, written in the same transaction as the record (no `keyPolicy` inside transactions, rob#46); read with a strong `get` before appending |
| CES `change` | `issueId` s, `projectId` s, `releaseId` s, `verdict` s, `rank` i, `reasons` s, `lastProdAt` s | `by-release` (`releaseId` / `rank`), `by-project` (`projectId` / `lastProdAt`) | panel boot in one read; release views |
| CES `release` | `releaseId` s, `projectId` s, `name` s, counts i…, `gateReady` boolean | `by-project` (`projectId` / `name`) | posture, recomputed from `change` rows by key, never a read-modify-write counter |
| KVS `gov:<hour>:<shard>` | points spent, sharded per invocation class | — | Tier 1 self-accounting without one hot key (rob#53) |
| KVS `gov:state` | `{state, pausedUntil, r, q, t, lastReason}` | — | quota state from headers |
| KVS `cfg:policy`, `cfg:budgets`, `role:<accountId>`, `wm:*`, `mig:*`, `failed:<id>`, `job:<id>`, `nar:<release>:<scopeHash>:<evidenceHash>:<model>`, `perm:<accountId>:<issueId>` (TTL + `expireTime` check, rob#47), `usage:llm:<month>` | | | config, roles, watermarks, dead letters, narrative cache scoped by visibility, permission cache |
| secret store `whsec` | via `kvs.setSecret` | — | the CI signing secret (sec#22) |

### C.2 Async topology

```
Jira events ─ trigger onIssueEvent ── filters relevant items, pushes ids only, returns ─▶ queue facts
                                      (concurrency key = "issue:<issueId>", limit 1)
            ─ trigger onInstalled / onUpgraded ─▶ queue backfill / migration (InvocationError retry on 401/403)
CI/CD ──────▶ webtrigger ingestDeployment (static outputs)
              verify HMAC over the raw body + timestamp window → deploymentId nonce (FAIL_IF_EXISTS)
              → push ids → output "accepted" | "duplicate" | "unauthorized" | "invalid"   [≤ 10 virtual s]
                 ─▶ queue attestations (key "issue:<id>", limit 1; one event per issue key)
                       → chain append + verdict + release posture → Realtime publishGlobal (ids only)
                       → queue stamps (key "stamp:<issueId>", delayInSeconds coalescing window)
                            → ONE ADF comment per issue per window listing its deployment ids;
                              stamp ledger (deploymentId → commentId); after an ambiguous 5xx, read
                              the comments before re-posting; per-issue write pacing
scheduled reconcile (hour) ─▶ queue sweep (key "sweep", limit 1): incremental JQL from the watermark,
                              budget-gated via the governor, continuation events, delay to the next budget
                              slot or reset
admin "add project" / upgrade ─▶ queue backfill (key "backfill", limit 2): chunked by remaining time,
                              re-enqueue with delayInSeconds when over budget or walled
upgrade / installed ─▶ queue migration (key "migration", limit 1): v1 keys → chain appends (storage only),
                              per-issue lock shared with the facts queue (same key "issue:<id>")
resolver ws.requestNarrative ─▶ queue llm (key "llm", limit = RPM pacing): consumer timeoutSeconds 600 →
                              chat() forced tool → validate → store draft (scoped) → Realtime to the requester
scheduled privacy (day) ─▶ queue privacy (key "privacy", limit 1): batches ≤ 90, one in flight,
                              Retry-After → erase or refresh names
```

Every consumer is idempotent on fact identity, never on `jobId` or `eventId` (rob#20). Consumers watch
`invocationRemainingTimeInMillis()` (rob#6) and checkpoint before the deadline. A wait longer than 10 virtual seconds
goes back to the queue (`InvocationError{retryAfter}` or a fresh push with `delayInSeconds`), never into an in-function
sleep. Failed items carry a bounded attempt counter in `retryData` or storage and are dead-lettered to `failed:<id>` with
their reason.

### C.3 Resolver catalogue

The refusal shape is stated: `{ok:false, error:{code:"FORBIDDEN"|"NOT_FOUND"|"INVALID"|"RATE_LIMITED"|
"BUDGET_EXHAUSTED"|"DEGRADED", message}}`. An issue the caller cannot see answers `NOT_FOUND`, so the response is no
existence oracle. Identity comes from `context.accountId` only (sec#1). `extension.*` ids are NOT proof of permission:
the harness passes whatever extension the caller supplies (a stated emulator choice, BRIEF §2.4 item 3).

| key | surface | input | authorization (server-side) | Jira / storage calls (golden) | failure handling |
|---|---|---|---|---|---|
| `panel.bootstrap` | issue panel | `{issueId}` (or the extension issue) | BROWSE on that issue id (cached ≤ 300 s per installation + account + issue) OR the auditor role (evidence only, no content) | perm-cache `get`; on a miss, asUser `GET /issue/{id}?fields=id` (404 means no); `change` + `evidence` reads | Jira 5xx: stored evidence + `degraded:"jira"`. Paused: `degraded:"quota"` |
| `ws.overview` | workspace | `{projectId?, cursor?}` | regulated projects; counts are team totals | `release` by-project query | stable app cursor, never a KVS cursor (rob#57) |
| `ws.release` | workspace | `{releaseId, sort, cursor}` | as panel, per change; hidden ones counted | `change` by-release (≤ 100/page); one asUser browse check for the page's issue ids (search `id in (…)`) | pages of 50 |
| `ws.change` | workspace | `{issueKey}` | as panel | key → id (asUser), chain read | |
| `ws.exportPack` | workspace | `{releaseId}` | as release; ≤ 6 per person per installation per virtual hour | chains of visible changes | `RATE_LIMITED` with `retryAt`, no Jira call |
| `ws.requestNarrative` | workspace | `{releaseId}` | auditor or release viewer; per-user cap; installation token budget | cache lookup; push to `llm` | `BUDGET_EXHAUSTED` / `RATE_LIMITED` |
| `ws.getNarrative`, `ws.publishNarrative` | workspace | `{releaseId, draftId}` | publish needs the auditor role (confirmation) | | |
| `ws.realtimeToken` | workspace | `{}` | any | `signRealtimeToken` subscribe-only, claims `{accountId}` from context | forged claims in the payload are ignored |
| `admin.getConfig`, `savePolicy`, `listRoles`, `grantRole`, `revokeRole`, `setSigningSecret`, `getWebhookUrl`, `setBudgets`, `getUsage`, `listFailed`, `retryFailed`, `discardFailed`, `getMigration` | admin page | per key | compliance-admin (stored) OR Jira ADMINISTER, checked live (`mypermissions?permissions=ADMINISTER` asUser, cached ≤ 300 s); never from payload or display conditions | | `FORBIDDEN`, zero side effects |
| `audit.exportChains` | any surface (admin or auditor) | `{issueIds ≤ 100}` | compliance-admin or auditor | chain reads | the stated read interface the grader uses for chains |

Rovo: `get-change-evidence` (GET, input `issueKey`) returns the panel's view for the invoking person, using the
same rules. `open-review` (CREATE, input `issueKey`, `note`) needs the auditor role and writes a review record.

### C.4 Rate-tier dosing the app must implement (the stated policy, contract §4)

Facts the policy rests on:

- Tier 1 is one 65,000-point hourly pool per app, shared by every tenant (tiers#1, #2).
- It resets at the top of each UTC hour, with no carry-over (tiers#4).
- It is a hard wall (a stated simplification, tiers#8).
- `r` appears only past ~80 % usage (tiers#17).
- Storage is per installation, so there is no cross-tenant counter (tiers#35).
- RoA forbids an external coordinator (sec#20).

The design therefore needs self-accounting plus header reaction (BRIEF §6.2).

**Stated benchmark rules (contract §4 and TABLES; numbers are proposals):**

1. **Pool.** 65,000 points per virtual UTC hour, shared by the world's three installations AND the app's other
   installations outside the world. Their draw is invisible except through headers: about 20,000 points in a normal
   hour, and at times far more.
2. **Costs** come from the TABLES cost table. It follows Atlassian's published model (base 1, +1 per core object, +2
   per identity or permission object, writes 1) with per-endpoint object definitions. Examples: search/jql
   1 + issues returned; changelog bulkfetch 1 + issues whose changelog appears; permissions/check 1 + 2 per entity
   checked. Atlassian publishes no catalogue (tiers#5, #6), and the contract says so.
3. **Background budget per installation.** Background work (backfill, reconcile, migration Jira reads, privacy
   lookups) spends at most the installation's configured budget per virtual hour: default **4,000**, set by a compliance
   admin in the console, range 1,000–20,000. A change applies within 5 virtual minutes. Interactive work (resolvers,
   web trigger, Rovo) is not budgeted but counts toward the pool.
4. **Smoothing.** In any 10-virtual-minute window, background spending ≤ **25 %** of the hourly budget (Atlassian:
   "Spread your requests evenly throughout the hour", tiers#23).
5. **Near limit.** When `RateLimit` carries `r` < 15 % of `q`, background work pauses until the reset. Interactive
   work continues.
6. **Wall.** After a 429 with a quota-class reason (`jira-quota-global-based`, `jira-quota-tenant-based`, an unknown
   reason, or the gateway variant without Retry-After), the installation makes no backend product call until the reset.
   Surfaces keep serving stored data marked `degraded:"quota"`. Deferred work resumes after the reset.
7. **Bursts.** At most **10** backend product requests in flight per installation. After `jira-burst-based`, that
   endpoint waits at least Retry-After (tiers#20).
8. **Per-issue writes.** 20 per 2 s and 100 per 30 s (tiers#13); stamps coalesce.
9. **Retries.** Repeat a request only after the signal (Retry-After; `X-RateLimit-Reset` when Retry-After is absent;
   the hour reset for quota). A write whose outcome is unknown (5xx or timeout) is never repeated before checking
   whether it was applied. At most 4 attempts per request. Jitter is allowed, never required.
10. **Headers.** The emulator sends the documented grammar: comma-separated policies in any order, an optional `r`,
    mixed `Beta-` and enforced prefixes, ISO times with or without seconds (tiers#18, #19). Bridge `requestJira`
    responses carry the same headers (FRGE-1923) and are EXEMPT from points, a stated assumption from staff
    statements (tiers#24, #25), though subject to the burst buckets.
11. **Per-person throttle.** Expensive interactive actions are limited: export ≤ 6 per person per installation per
    virtual hour. This is the "one user clicking a button exhausted the global quota" incident from tiers#27 and
    T99654.

**Golden mechanism.** One `governor` module wraps every backend `requestJira`:

- It estimates cost from the table and admits background calls against the installation's hour ledger, which is a
  sharded KVS counter with conditional increments.
- It parses `RateLimit` and `RateLimit-Policy` (and the `Beta-` and `X-RateLimit-*` variants) into `gov:state`.
- It enforces the in-flight cap with per-invocation limits: concurrency keys bound invocations, and each invocation
  allows ≤ 2 in flight.
- It returns `PAUSED` instead of calling when walled.
- Interactive callers bypass the budget but honour a pause by serving stored data.
- User-context reads in the surfaces use bridge `requestJira` (free) where the data is the user's own view, for
  example issue summaries for display. Authorization decisions are never made in the browser.

### C.5 LLM features

**Control narrative per release** (auditor-facing). The flow:

1. `ws.requestNarrative` checks the per-user cap (default 5 per virtual hour) and the installation token budget
   (default 5M tokens per month; the admin sets it).
2. It computes the cache key: installation + **visibility scope** (the requester's visible change set; auditor or
   not) + evidence-state hash + model.
3. On a cache hit it returns the draft (0 LLM calls). Otherwise it pushes `{releaseId, requester}` (ids only, rob#16)
   to `llm`.
4. The `llm` consumer (`timeoutSeconds` 600) builds the prompt ONLY from what the requester may see: evidence records,
   verdicts the app computed, and issue summaries the requester can browse, delimited as untrusted data. It calls
   `chat()` with an `active` model from `list()` at call time, a forced tool `record_narrative`
   (`{summary: string, findings: [{issueKey, note}], citations: [evidenceRef]}`) and `max_completion_tokens` ≤ 4,096.
5. It validates the arguments: types; keys ⊆ release ∩ visible; citations ⊆ visible evidence; digits in `summary` ⊆
   numbers the app computed.
6. It stores the draft scoped to the visibility scope and publishes "ready" to the requester's Realtime channel (claims
   derived server-side). The UI shows it labelled AI-generated. Publishing (making it visible to every auditor) needs
   an auditor's explicit click.

**Draft all.** An auditor bulk action queues every release of a project. The app must pace to ≤ 100 requests per
minute per installation and ≤ 500k tokens per minute per model (stated estimator), and the batch must complete.

**Failure classes** (stated shapes in TABLES): refusal, 403 `FORGE_LLMS_MODEL_FORBIDDEN`, 429, 500, a stream ending
without `finish_reason` (if `stream()` is used), an empty `end_turn`. Each gives a `narrative-error` state with a reason
class. The rest of the app keeps working. Retry rule: no LLM request from the installation for 60 virtual s after a
429, and ≤ 2 retries per job.

**Errors.** Use `err.name === 'ForgeLlmAPIError'` or `instanceof ForgeLlmError` (llm#36). The class is a
discoverable-trap, not stated.

### C.6 Front-facing Custom UI surfaces and their boot budget

Both surfaces call `view.theme.enable()`, use `--ds-*` tokens, and paint their own surface. They pass in light and
dark, at 320 px (panel) and 1280 px (workspace), under the production CSP. The design is "pretty" by stated,
measurable rules:

- tokens and ≥ 4.5:1 contrast;
- a posture chart whose bars are proportional within 1 px;
- loading, empty and error states (boot#28);
- no clipped numbers;
- screenshots and video published for humans.

**Boot protocol: hold-and-release waves** (bootspeed §3.2). Host-local operations (`getContext`, `theme.enable`,
`createHistory`, flags) are free. Backend-bound operations (`invoke`, `requestJira`, Realtime subscribe, flags init)
are queued and released in waves when the frame settles. READY means seeded content is shown, never a marker. The
budgets are *proposals*; calibration freezes them:

| metric | issue panel | workspace |
|---|---|---|
| SHELL at wave 0 (also while Realtime and flags are held) | yes | yes |
| backend waves to READY | ≤ 1 | ≤ 2 |
| backend-bound ops before READY | ≤ 2 | ≤ 3 |
| app-origin gzip -9 bytes before READY (platform-injected excluded) | ≤ 110 KB | ≤ 230 KB |
| app-origin requests / initiator depth | ≤ 5 / ≤ 3 | ≤ 6 / ≤ 3 |
| bootstrap resolver: sequential outbound rounds; calls for a 3-record vs a 120-record chain | ≤ 2; equal | ≤ 3 |
| LLM calls, Jira writes, queue pushes during boot | 0 | 0 |
| repeated bootstrap invokes on in-session navigation; invokes while idle | — | 0; 0 |
| bootstrap invoke answered 429 (`rateLimitProperties`) | shell stays; exactly 1 retry at or after `rateLimitReset` | same |
| CSP violations; non-app, non-platform origins | 0; 0 | 0; 0 |

The panel is the boot-speed surface that matters: it renders on every issue view, so its budget is the tighter one.

### C.7 The UI Kit 2 admin panel

`jira:adminPage`, `render: native`, `useAsConfig: true`, `@forge/react` ≥ 10 on React 18.3.1, `resource` a FILE
(uikit#19). Navigation is by `Tabs` (`testId`s stated) or the Preview `Router`; sidebar subpages are Custom UI only
(uikit#20). The six tabs:

| tab | components | actions (resolver payloads stated) | graded semantics |
|---|---|---|---|
| Policy | `Form` + `useForm`, `Select` (approval field, from the site's fields), `Select isMulti` (regulated projects), `CheckboxGroup` (material fields), `Toggle` (deployer SoD), `Textfield type="number"` (stamp window) | `admin.savePolicy {approvalFieldId, regulatedProjectIds[], materialFieldIds[], deployerSod, stampWindowMin}` | Valid input gives exactly one invoke. Invalid input (window outside 1–60) gives the stated `ErrorMessage` text and 0 invokes. A double Save gives 1 invoke. `onSubmit` gets no data, so `handleSubmit` is the gate (uikit#28). |
| Roles | `DynamicTable` (User, Role, Granted by, Granted), `UserPicker`, `Button`, `Modal` | `grantRole {accountId, role}` and `revokeRole {accountId, role}` | Rows equal the stored roles. Sorting by Granted is chronological both ways; a display-date cell key mis-sorts under the ADS collator (uikit#11). UserPicker sends the accountId (`.id`, uikit#29). Remove needs the confirm Modal. |
| Integrations | `Text` (web trigger URL from `webTrigger.getUrl`), `Textfield type="password"`, `Button` | `setSigningSecret {secret}` | Shows only "set on <date>". Rotation works. |
| Usage & budgets | `Lozenge` (quota state), `DynamicTable` (points per hour, 429s by reason, LLM tokens, credits, cost), `Textfield` (budgets) | `setBudgets {backgroundPointsPerHour, llmTokensPerMonth, narrativesPerUserHour}` | Numbers rendered from `admin.getUsage` (their truth is graded in T and G) |
| Work queue | `DynamicTable` (item, reason, attempts), `Button` Retry or Discard, `Modal` | `retryFailed {id}`, `discardFailed {id}` | Discard needs confirmation |
| Migration | `ProgressBar`, `Text` | — | migrated / total / state equal the truth |

**Efficiency (stated):** the first ForgeDoc is non-empty (a loading state) before any invoke resolves; ≤ 2 invokes
before the Policy form is complete; ≤ 4 reconcile commits per user action; no `onError` and no `BridgeAPIError`
(uikit#16).

### C.8 Web trigger and integrations

The scheme is stated in full in contract §5, because the platform has no documented authentication (sec#15, #17):

- `POST <url>` with headers `x-webtrigger-timestamp: <RFC 3339 UTC>` and
  `x-webtrigger-signature: sha256=<hex HMAC-SHA256(secret, timestamp + "." + rawBody)>`.
- Header names are case-insensitive, and each header value is an array (the request shape, sec#3 web-trigger request).
- Accept only when |now − timestamp| ≤ 300 virtual s, the signature matches over the RAW body, and the body parses.
  Do nothing before verification.
- `deploymentId` is the nonce: a second accepted delivery of the same id has no effect and answers `duplicate`.
- Static outputs: `accepted` (202), `duplicate` (200), `unauthorized` (401), `invalid` (400).
- The secret is the one a compliance admin set in the console, stored with the secret store, 32–64 bytes. After
  rotation only the new secret is valid.
- The platform's `request.authentication: hmacSharedSecret` (tooling shipped, undocumented, enforcement unverified)
  is neither required nor penalised.

The **sender behaviour** is stated as part of the integration contract, not as a fault list:

- Senders deliver up to 2 attestations per second per installation.
- A delivery not acknowledged within 10 virtual seconds is retried with the same body, re-signed at send time.
- A sender may deliver the same attestation more than once.
- Issue keys that match no regulated issue may appear.

**Payload.** `{deploymentId, environment: "production"|"staging", pipeline, commit, issueKeys[], deployedBy:
accountId|null, deployedAt}`. `pipeline` is untrusted text and may contain markup.

### C.9 Brownfield: live v1 → v2 migration

Two installations ran **v1** (contract §10, appendix V1):

- v1 stored `ev1:<issueId>` → `{approvals:[{changelogId, by, at}], deployments:[{deploymentId, env, by, at}]}` and
  `cfg1` → `{webhookToken, approvalFieldId, regulatedProjectIds, materialFieldIds}` in plain KVS. Storing the token in
  plain KVS was v1's own flaw.
- v1 pushed `{type:"approval"|"deployment", issueKey, …}` events to queue `cp-events`.

At the start of the scoring scenario the harness **upgrades** both installations to the entrant's v2 code. The
upgrade is major, since v2 adds `llm`; the harness delivers `avi:forge:upgraded:app` (rob#32 and the life-cycle doc)
while v1 events are still queued.

Stated requirements:

1. Every v1 fact appears exactly once in the v2 chains. v1 approvals ARE Jira changelog facts, so the backfill sees
   them too and identity is (`kind`, `ref`, `issueId`). v1 deployments exist ONLY in v1 storage, so losing them is
   data loss.
2. Queued v1 events reach the consumer v2 declares for `cp-events`.
3. The v1 policy becomes the initial v2 policy.
4. The v1 plain-KVS token is removed once migration completes and is never shown or logged.
5. `ev1:*` keys are deleted only after their facts exist in v2.
6. Migration completes within 2 virtual hours, while live facts for the same issues keep flowing.

---------------------------------------------------------------------------------------------------------------------

## D. The world

### D.1 What the contract says about the world (guarantees and facts, never a fault schedule)

- "The app is installed on three Jira sites at once, in the same Forge runtime processes. Module state may persist
  across invocations and across installations, or be reset, at any time; warm reuse WILL be exercised." (rob#11,
  tenant-data-isolation.)
- "Two installations already run v1 (§10). The harness upgrades them to your code when the scenario starts. A third
  site installs your app during the run."
- "Scoring scale, approximate: about 12,000 / 2,500 / 400 issues, of which about 3,000 / 700 / 120 changed in the
  180-day evidence window; dozens of releases; tens of people. The dev site is about a tenth of that. Seeds differ;
  ids, keys, field ids and names differ."
- "Issue and project ids are unique per site only. Some people have accounts on several of the sites. Roles are per
  installation."
- "Some issues are protected by issue security. Some people cannot browse some projects. Permissions, policy, scope,
  budgets and the signing secret can change at any time; each guarantee says how quickly the app must follow."
- "CI/CD senders behave as §5 states."
- "Other installations of the app draw on the same Tier 1 pool (§4). Other apps on a site share that site's burst
  buckets."
- "A Jira write answered with a 5xx, or one that times out, may or may not have been applied." (Not in Atlassian docs;
  stated because the app's exactly-once guarantees depend on it.)
- "Accounts can be closed; the Personal data reporting API says so."
- "The platform behaves as documented in the platform reference pack, with the deviations listed in §11."

### D.2 Dev site against scoring sites

| | dev world | scoring world (×2 seeds) |
|---|---|---|
| installations | dev-a (~1,200 issues, v1 data ~300 records), dev-b (~200 issues, fresh) | alpha (~12,000; v1 ~2,500 records), beta (~2,500; v1 ~600), gamma (~400; installs at virtual +2:10) |
| other installations' draw | high: crosses 80 % in dev hour 2 and walls in dev hour 3 of the dev timeline, so near-limit and wall states are observable while developing | nominal ~20,000/h; one scripted surge (§D.3) |
| platform semantics | all active at lower intensity; **every fault CLASS occurs at least once** over the dev timeline (the 1.0 fairness invariant), with no labels and no menu | all active, heavier |
| LLM | scripted, a seeded mix of answers including the failure classes; **no printed script** (1.0 printed one, and Pareto used it, frontier-behaviour row 16) | per-release scenario mapping (§D.3) |
| tools | `forge-dev invoke · events · webhook (signs with the secret you set in your console) · scheduled · clock (advance virtual time) · serve (Custom UI + UI Kit host) · ledger (points and 429s per installation-hour) · llm · kvs · users · install --fresh · upgrade · reset` | — |

### D.3 The private fault schedule (DESIGN-only; never in the public text)

Per scoring world, on the virtual timeline (proposal):

| virtual time | phase | what the harness does |
|---|---|---|
| 0:00 | A | Upgrade alpha and beta (v1 data present; ~40 v1 events queued on `cp-events`). Probe: admins set the signing secret through the console. Hourly reconciles at fixed per-installation minutes (:10, :25, :40). Panel storm: 1,000 panel views per installation-hour driven through `panel.bootstrap` as varied users (no browser). |
| 0:20–1:30 | A | Product events with the documented delivery semantics: duplicates, permutations, ~4 % never delivered, deliveries up to 180 s late, self-generated events after stamps. A burst-bucket squeeze on one alpha endpoint (other apps' traffic). One reconcile duplicated (concurrent), one skipped. |
| 1:30–1:40 | **B** (replayed ×3 schedules from a snapshot) | **Release train** on alpha: ~420 attestations in ~4 virtual min. Two "umbrella" changes are referenced by ~60 deployments each. Sender retries after a 10 s non-ack; 6 % duplicates; 3 replays with a stale timestamp. Concurrent product events for the same issues (approvals, a revocation, material edits). Ambiguous 503-after-commit on 8 stamp writes. Two attestations naming unknown keys (poison). |
| 2:00–3:00 | C | World changes: the policy changes through the console (material fields, deployer SoD); a revocation; 3 issue deletions; one user loses browse on a project; one account is closed (privacy); the scope expands (a beta project becomes regulated); the alpha background budget drops to 2,500; the signing secret rotates. Gamma installs at 2:10 with a 10 virtual min asApp 403 window. Unknown-key issues are created, so admin retry can succeed. One deliberately poisoned event body fails every attempt. A consumer is killed mid-chunk at its timeout. |
| 3:00–4:00 | D | Phantom surge: other installations push the pool past 80 % at ~3:25 (`r` appears) and exhaust it at ~3:40 (quota 429s until 4:00; the first 429 for gamma is the gateway variant without Retry-After). Interactive probes during the wall. |
| 4:00–5:00 | E | LLM battery: a clean release, invented digits, a hidden key, an injection-compliant answer (asserts "all compliant", emits a URL, asks for the secret), refusal, malformed arguments, 429, 500, a truncated stream (only if `stream()` is used), an empty `end_turn`. A(wide) then B(narrow) on one release. Draft-all on alpha (36 releases). The budget kill switch (admin sets a low budget). `claude-haiku-4-5-20251001` turns `deprecated` at 4:30. Rovo battery. Security battery (admin bypass, IDOR, path injection, web-trigger forgery and replay, Realtime isolation, cross-tenant). Export button-masher (20 clicks). |
| 5:00–6:00 | F | Daily privacy trigger (mock: one `closed`, one `updated`, a 429 with Retry-After on the second request). Final reconcile with nothing new (expects 0 writes). Final checkpoint exports. |
| browser | UI | Issue panel cold boots (short chain, 120-record umbrella chain, restricted viewer, auditor) × light/dark. Workspace cold boot, deep links, navigation, a live posture update during a mini-burst, the narrative flow over Realtime. UI Kit admin flows (performed during A, C and E). Screenshots and video. |

### D.4 Determinism and load-robustness

- **Virtual time everywhere.** An in-child agent virtualises `Date`, timers and `timers/promises`, and answers
  `getRemainingTimeInMillis` from virtual elapsed (machinery §4.3). Proxied calls charge the stated latency model.
  App CPU time is free (lenient, never unfair). An invocation that runs 10 REAL minutes is treated as timed out; only
  an infinite loop gets there, a 600× margin over typical work.
- **Deterministic concurrency** (machinery §4.2): the proxy is the only I/O channel (sandbox fence). Every proxied
  request parks until the seeded scheduler grants it. Steps advance when every in-flight invocation is parked,
  finished, or waiting on a virtual timer. Phase B runs under three policies: **serial**, **reads-first adversarial**
  (all parked reads are granted before any write, which exposes read-modify-write deterministically) and **seeded
  random**. `Math.random` is seeded per invocation (stated).
- **Boot** is counted in waves, bytes and requests, never in milliseconds (bootspeed §3).
- **UI Kit** is graded on ForgeDoc snapshots, the bridge-call log and quiescence measured in host ticks.
- **Realtime** checks are event-driven: the host records delivery, then quiescence. There are no wall-clock waits that
  can turn load into failure.
- **Harness evidence is never app evidence.** A click the harness could not deliver, an unmodelled endpoint or
  component, or a real-time watchdog gives `unavailable` or `held`, never a 0 (1.0 defects A and B).

---------------------------------------------------------------------------------------------------------------------

## E. Contract outline (`CHANGEPROOF-CONTRACT.md` ≈ 24 KB + `CONTRACT-TABLES.md` ≈ 5 KB)

The rule for what the text holds:

- It states GUARANTEES (outcomes), the benchmark's own numbers, every harness deviation, and world facts that no
  Atlassian page documents.
- Documented platform behaviour is NOT restated. It lives in `$FORGE_KIT/reference/platform/*.md` (verbatim excerpts,
  each with URL, "Last updated" and fetch date) and in the typings, schema and OpenAPI.
- The contract never says which fault happens when, how often, or to what.

| § | title | key sentences (abridged) | KB |
|---|---|---|---|
| 0 | What you build | "Changeproof keeps change evidence for regulated teams on three Jira sites at once. The harness installs your app, upgrades two v1 installations, runs it for about six virtual hours on a seeded world, and grades it by running it: product events, attestations, scheduled runs, resolver calls as many people, the Rovo actions, and every surface in a browser and in the UI Kit host. Nothing is deployed." | 0.9 |
| 1 | Evidence model | Facts, identity, the chain and the hash string (exact), verdict rules, posture, reason codes, time format (§C.1 verbatim). "`audit.exportChains({issueIds})` (≤ 100 ids) returns every chain; auditors and compliance admins may call it." | 3.6 |
| 2 | Modules, roles, RoA | The module table (keys and roles = WHAT; HOW is discoverable). "The app must be eligible for Runs on Atlassian: no external domains, remotes, providers or Connect modules; web triggers are static." Roles: compliance-admin (granted by admins; every holder of Jira ADMINISTER is one implicitly), auditor. "Every resolver returns a value and never throws." "Request only the scopes your calls need (OpenAPI per call)." "Preview modules are demanded as documented on 2026-10-09." | 1.9 |
| 3 | Guarantees | **G1 Exactly once**: every fact of the evidence window, every accepted attestation and every v1 record becomes exactly one record. **G2 Integrity**: chains are gap-free and fork-free; records are never rewritten. **G3 Freshness**: a production attestation is reflected (record and verdict, panel and workspace) within 120 virtual s at up to 2 per s per installation; a delivered product event within 120 s of delivery; anything events miss by the next hourly reconcile. **G4 Backfill bars**: alpha 3 h and beta 1.5 h after upgrade, gamma 1 h after install, a newly regulated project 2 h after it is added. **G5** A reconcile with nothing new writes nothing. **G6** Policy changes apply within 15 min; deletions append a `deletion` fact and leave posture; a person who loses browse sees no evidence of that issue 300 s later (unless auditor). **G7** Each production deployment of an approved change is stamped within the stamp window (default 10 min) by an ADF comment from the app naming its deployment id; every id appears in exactly one stamp comment, and one comment may list several. **G8** Anything that cannot be processed appears, after at most 5 attempts, in the console's Work queue with its reason; nothing is dropped silently; admins can retry. **G9** Budgets and pacing (§4). **G10** Migration (§10). **G11** Isolation (§5). **G12** When Jira or the pool is unavailable, surfaces serve stored evidence marked `degraded`. | 3.2 |
| 4 | Tier 1 and platform budgets | §C.4 rules 1–11 verbatim, with "background" and "interactive" defined; "never wait longer than 10 virtual s inside an invocation"; "at most 10 product requests in flight per installation"; the usage resolver schema (`admin.getUsage`: points this hour, budget, state, 429s by reason, LLM tokens, credits and cost) "accurate to ±2 %". | 2.5 |
| 5 | Security | Refusal shape; "resolvers are callable directly by anyone who can load the module"; "extension ids are not proof of permission"; "the app user sees everything, including issue-security levels (emulator assumption)"; permission verdicts may be cached ≤ 300 s; the auditor rule (evidence yes, content of unbrowsable issues no); the full web-trigger scheme and sender behaviour (§C.8); secrets (console-entered, secret store, presence-only read-back); "no data of one installation ever reaches another"; user-controlled fields (summaries, display names, pipeline, commit, LLM output) are rendered as text; personal data stored (display-name snapshots) with erase and refresh semantics. | 3.1 |
| 6 | Forge LLM | The narrative feature (§C.5); the tool name and JSON schema; "invalid arguments are an error"; "only numbers and keys you computed or can show"; "treat model output and issue text as untrusted"; confirm-to-publish; AI label `data-ai-generated`; models from `list()` at call time, `deprecated` never called; caching semantics; budgets and kill switch; the retry rule; latency envelope "a narrative call may take up to 170 virtual s"; "opening a page makes no LLM call"; credit rates by tier (Haiku 10 / Sonnet 30 / Opus 50 credits per 1M tokens, $0.10 in and $0.50 out per credit; an assumption for unpublished models). | 2.1 |
| 7 | Custom UI | Panel and workspace hooks (`data-testid`, `data-verdict`, `data-reason`, `data-seq`, `data-ref`, `aria-sort`, …); routes `/release/<id>` and `/change/<key>`; sort orders; page size 50; live posture "without reload, through Realtime, no polling"; the boot rules and numbers of §C.6 with the wave protocol, free and backend-bound operations, gzip level and exclusions; CSP and token rules; widths. | 2.6 |
| 8 | UI Kit admin | `render: native`, tabs and `testId`s (only components that deliver them, uikit#31), form payloads, validation texts, roles-table sort semantics, efficiency numbers, "no pixels are graded", React 18 and classic-pragma `.jsx`. | 1.7 |
| 9 | Rovo | The two actions (inputs, outputs, `actionVerb`), agent, `rovo:mcp` exposing `get-change-evidence`. | 0.6 |
| 10 | v1 | The v1 storage and event shapes (§C.9) and the migration requirements. | 1.0 |
| 11 | Harness deviations and choices | Virtual time and the latency-model table; `Math.random` seeded; ≤ 32 async invocations per installation at once; timers pending at return are frozen and may resume in a later invocation or never; redelivery after a throw or timeout at 1, 2, 4, 8 min then every 15, to 24 h; query results may omit writes from the last 2 virtual s; KVS and Jira cursors are valid only within the invocation that produced them; the first 10 virtual min after an install answer asApp calls with 403; scheduled runs fire at a fixed minute per installation and a run can be skipped (stated, from reported production behaviour); every limit in TABLES is enforced; the LLM is scripted; screenshots are evidence, not grades. | 1.8 |
| T | TABLES | The cost table per endpoint, header sets per 429 kind (quota, burst, per-issue, gateway, unknown reason, 503), the latency model, LLM error shapes and the token estimator (`ceil(utf8_bytes/4)` + 20 per message), boot numbers, the KVS and queue limits enforced. | ≈ 5 |

**Total public input:** prompt ≈ 3.2 KB (with the Definition of done and the score bands in words) + contract ≈ 24 KB
+ tables ≈ 5 KB + starter ≈ 4.5 KB ≈ **36 KB**. The prompt tells the entrant: "The platform reference pack is
for lookup: grep it, do not read it whole."

---------------------------------------------------------------------------------------------------------------------

## F. Check registry

### F.0 Conventions

- "Export" means `audit.exportChains` called by the harness as a compliance admin at checkpoints. These calls are
  flagged as harness calls and excluded from every budget.
- "Gateway" is the points and limits log, one row per product call: `{t_virtual, installation, principal, method,
  path, status, points, policy, reason, headers}`.
- Every fraction row is the measured fraction.
- Correctness rows take the WORST of 2 worlds, and C rows also the worst of 3 schedules. X rows take the mean.
- Weights are the tier weight split equally over the tier's rows unless stated.
- **C** marks a critical. A critical fires ONLY on its named consequence, observed (1.0 defect B).

### F.1 Inner tiers (weights sum to 1.00)

| L 0.05 | E 0.13 | C 0.08 | R 0.11 | T 0.13 | S 0.15 | G 0.09 | B 0.07 | U 0.07 | V 0.03 | K 0.06 | D 0.03 |
|---|---|---|---|---|---|---|---|---|---|---|---|

**L: deployability and currency (5 rows, 0.010 each)**

| id | C | measured (load-robust) | anchor |
|---|---|---|---|
| `l_deployable` | C | Kit lint run twice with identical results (else every L row is `unavailable`). Zero errors from the CLI's client half (the pinned packages, byte-identical to CLI 14.1.0), the pinned runtime flag `["sandbox","nodejs18.x","nodejs20.x"]`, and the **measured server-rule pack** (each rule carries its wolfaenpak receipt). The last stage is reached. Consequence "deploy blocked", severity 0. | P done 1 |
| `l_bundles_load` | C | Share of manifest functions that bundle as `forge deploy` does and load with the handler exported. Severity transform 0 below 0.4, else 1. | P done 1 |
| `l_lint_warnings` | | 1 − 0.2 per distinct warning. | P done 1 |
| `l_scopes` | | Declared = needed: per observed call, the OpenAPI OAuth2 alternative (classic where it exists, else the granular set), plus `storage:app`, plus `report:personal-data`. −0.25 per missing or extra. | §2 |
| `l_platform_current` | | Fraction of 10 manifest conditions: each required module in its current form; RoA-eligible (no external domains, remotes, providers, Connect modules or dynamic web triggers); one `llm`; ≤ 1 `rovo:mcp`; ≤ 5 scheduled triggers and ≤ 1 fiveMinute; nodejs22.x/24.x; adminPage `render: native` with a file resource; one `jira:globalPage`; consumer forms the emulator supports (a valid-but-unsupported form is impossible, see §I). | §2 |

**P: diagnostic presence (weight 0; feeds bands and roots)**

| id | passes when |
|---|---|
| `p_pipeline` | ≥ 1 export record from each source: backfill, product event, attestation |
| `p_export` | `audit.exportChains` returns the stated shape for ≥ 1 issue |
| `p_panel` | the panel shows a verdict for ≥ 1 issue |
| `p_workspace` | the workspace shows posture for ≥ 1 release |
| `p_admin` | the admin page's first ForgeDoc is non-empty and has the Policy tab |
| `p_capabilities` | each of 9 capabilities is exercised once: an attestation accepted and recorded; a narrative delivered over Realtime; a policy saved through the console; a role granted through the console; a Rovo action answered; migration progressed; a privacy report sent; a stamp written; an export produced |

**E: evidence correctness (10 rows, 0.013 each)**

| id | C | measured | anchor |
|---|---|---|---|
| `e_backfill_complete` | | At each installation's bar (virtual), the fraction of its evidence-window facts present in the export. Lateness only: facts never recorded are priced by `e_no_evidence_lost`. | G4, §1 |
| `e_live_facts` | | Of the facts eventually recorded from live sources, the fraction first seen within their freshness bound. Export checkpoints every 30 virtual s through phases A–C. | G3 |
| `e_exactly_once` | | Fraction of facts with exactly one record across all chains. Sources of doubles: redelivery, sender retries, reconcile overlap, migration overlap. | G1 |
| `e_verdicts` | | Per change × 4 checkpoints (post-backfill, post-train, post-changes, final): verdict and reason set equal the oracle. | §1 |
| `e_posture` | | Per release × 4 checkpoints: counts by verdict and `gateReady` equal the oracle. | §1 |
| `e_idle_reconcile` | | Storage writes by the final no-change reconcile and its descendants: 0 → 1.0, ≤ 1 % of records → 0.5, else 0. | G5 |
| `e_change_propagation` | | Fraction of world changes converged in time: policy ≤ 15 min, revocation and material change ≤ freshness, deletion fact present and posture excludes the issue, scope expansion within its bar. | G6 |
| `e_stamps` | | Fraction of production deployments of approved changes whose id appears in a stamp comment (ADF, authored by the app) within the window. | G7 |
| `e_no_evidence_lost` | C | Final checkpoint: fraction *s* of all facts present. Fires only when *s* < 1, naming the missing facts. Factor 0.6 + 0.4·*s*⁴, continuous, so there is no one-fact cliff (1.0 lesson F). | G1 |
| `e_no_duplicate_stamp` | C | Fires only when some deployment id appears in ≥ 2 stamp comments on Jira, observed in the mock's comment store. Cliff: "duplicate side effect on a customer's Jira". | G7 |

**C: concurrency and throughput (4 rows: integrity 0.030, SLO 0.020, posture 0.015, ack 0.015)**

| id | measured | anchor |
|---|---|---|
| `c_chain_integrity` | After phase B under each of 3 schedules, every chain is checked: `seq` contiguous, `prev` links correct, hash recomputes per §1, no two records share a `seq` or a `prev`. Fraction of intact chains, worst schedule. | G2 |
| `c_freshness_slo` | Fraction of train attestations whose record AND verdict (`panel.bootstrap` sampled on 40 issues) are visible ≤ 120 virtual s after acceptance. Worst schedule. | G3 |
| `c_posture_race` | Release posture right after the train equals the oracle under every schedule (the lost-update detector). Worst. | §1, G1 |
| `c_ack` | Fraction of web trigger invocations returning within 10 virtual s during the train. | §5 sender |

**R: robustness (10 rows, 0.011 each)**

| id | measured | anchor |
|---|---|---|
| `r_deadlines` | Fraction of invocations NOT killed at their virtual timeout in nominal phases. The injected mid-chunk kill is excluded; its recovery is graded by E. | §11 |
| `r_retry_discipline` | Fraction of retry episodes that are compliant: no repeat of a refused request before its signal (Retry-After, `X-RateLimit-Reset` for the gateway variant, the reset for quota); no repeat of an unknown-outcome write without a prior check; ≤ 4 attempts. "Repeat" uses 1.0's §17.6 definition (same invocation lineage, method, path and body). | §4 |
| `r_no_long_waits` | Fraction of invocations whose in-function timer waits total ≤ 10 virtual s. | §4 |
| `r_poison` | Poison items appear in `admin.listFailed` with a reason after ≤ 5 attempts. Everything else completes. After the cause is fixed (the issue now exists), `admin.retryFailed` records them. | G8 |
| `r_schedules` | The duplicated concurrent reconcile adds no duplicate records; facts from the skipped hour are recorded by the next run; an overlapping run causes no double processing. Fraction of anomalies handled. | §11, G1 |
| `r_install_race` | Gamma's backfill completes by its bar despite the 403 window, with no retry storm (≤ 4 app retries per product event; no in-function loops). | G4, §11 |
| `r_deploy_midflight` | Fraction of queued v1 events whose facts are recorded. | §10 |
| `r_unawaited` | Fraction of invocations with no promised side effect pending at return (a queue push, storage write or product call still pending when the handler returned; the emulator freezes timers). | runtime ref |
| `r_platform_limits` | Fraction of invocations with zero KVS limit errors, zero KVS `RATE_LIMIT_EXCEEDED`, ≤ 100 log lines per runtime minute and ≤ 200 KB, and zero queue push errors (too many events, payload, rate, cyclic). | §11, TABLES |
| `r_self_events` | Self-generated events (from stamps) cause 0 Jira writes and 0 new facts. | G1 |

**T: Tier 1 dosing (11 rows, 0.0118 each)**

| id | C | measured | anchor |
|---|---|---|---|
| `t_bg_budget` | | Per installation-hour, background points ≤ configured budget plus one in-flight request, including after the mid-run change (allowed 5 min). Fraction of installation-hours. | §4.3 |
| `t_smoothing` | | Fraction of 10-virtual-minute windows in which an installation's background points ≤ 25 % of its budget. | §4.4 |
| `t_quota_wall` | | Mean of: product calls between the first quota 429 and the reset (0 → 1, beyond each invocation's first refused call); deferred work resumes and completes after the reset; interactive resolvers during the wall answer `degraded:"quota"` with stored data. | §4.6, G12 |
| `t_near_limit` | | Background points spent while the last seen `r` < 15 % of `q` = 0, and interactive calls continue. | §4.5 |
| `t_burst_discipline` | | Max in-flight product requests per installation ≤ 10 at every instant, and no retry of a burst-429 endpoint before Retry-After. Mean. | §4.7 |
| `t_per_issue_writes` | | Fraction of issues never receiving > 20 app writes in 2 s or > 100 in 30 s (refused writes included). | §4.8 |
| `t_interactive_served` | | Fraction of interactive invocations in nominal hours answering without error. Degraded answers count as served. | G12 |
| `t_backfill_bars` | | Per installation `min(1, bar / t_complete)`, continuous. | G4 |
| `t_user_throttle` | | Button-masher: ≤ 6 exports per person-hour served; extras answer `RATE_LIMITED` with no Jira call. | §4.11 |
| `t_usage_truth` | | `admin.getUsage` at 6 checkpoints, including during the surge, against the gateway's truth: points this hour (±2 %), quota state, 429s by reason. Fraction of fields. | §4 |
| `t_pool_exhausted_by_app` | C | A quota wall in a NOMINAL hour (other installations at their nominal draw). Only the app can cause it. Cliff: "every customer of the app loses Jira access until the reset". | §4.1 |

**S: security (17 rows, ≈ 0.0088 each)**

| id | C | measured | anchor |
|---|---|---|---|
| `s_admin_bypass` | C | Every admin resolver key invoked as plain user, project admin, auditor, and another installation's admin, through every module context that exposes it, with forged `role`/`isAdmin`/`accountId` in the payload. Fires on any observed KVS, secret, Jira or queue diff. | §5 |
| `s_admin_refusal` | | Same calls: exact refusal shape, and actions attributed to `context.accountId`. Fraction. | §5 |
| `s_roles` | | Only admins grant or revoke; self-grant is refused; Jira ADMINISTER is an implicit admin on a fresh install; a demoted Jira admin loses rights ≤ 300 s; roles are per installation for the same account. Fraction. | §2, §5 |
| `s_idor` | | Payload ids the caller cannot browse, forged extension ids, a check id differing from the sink id, path-injection ids (`../../rest/api/3/...`, sec S22) → `NOT_FOUND`, no data, no unintended endpoint. Fraction. | §5 |
| `s_permission_leak` | C | Hidden-issue canaries (summary and description text, keys) observed in any person-facing response, DOM, LLM prompt, Realtime payload or export. | §5 |
| `s_auditor_scope` | | Auditors see records and verdicts of every regulated issue, with a placeholder instead of the summary for unbrowsable issues. Non-auditors without browse see nothing. Fraction. | §5 |
| `s_permission_ttl` | | After a user loses browse at *t*, no evidence of those issues is served after *t* + 300 s. Fraction of probes. | G6, §5 |
| `s_webtrigger_auth` | | Battery: unsigned; wrong signature; tampered body; stale and future timestamps; case-varied names; multi-value arrays; missing timestamp; valid with insignificant whitespace in the raw body. Each answers the stated output. Fraction. | §5 |
| `s_webtrigger_forgery` | C | Fires on any side effect (record, push, Jira write) observed from a rejected request. | §5 |
| `s_webtrigger_replay` | | A signed request sent twice, sequentially and concurrently, and a re-signed retry: exactly one set of records, and the replay answers `duplicate`. Fraction. | §5 |
| `s_secret_storage` | | The console secret is stored through the secret store (the emulator sees `setSecret`); the v1 plain token is gone after migration; `getConfig` shows presence only. Fraction. | §5, §10 |
| `s_secret_exposed` | C | Fires on the secret canary or the v1 token observed in any response, log, prompt, Realtime payload, egress or bundle. | §5 |
| `s_cross_tenant_leak` | C | Fires on one installation's canary (names, summaries, roles, cached verdicts) observed in another installation's outputs, storage, logs or prompts. The world seeds overlapping issue ids and accounts that span sites. | G11 |
| `s_xss` | | Payloads in summaries, pipeline, display names and LLM output: no script execution (a canary global stays unset), no `on*` attributes, no `javascript:` hrefs, no `unsafe-inline`/`unsafe-eval` script relaxations. Fraction. | §5, §7 |
| `s_rovo_inputs` | | A forged `accountId` input, an invisible key, a non-auditor calling `open-review`: identity from context, refusal, no write. Fraction. | §9 |
| `s_realtime_isolation` | | A second user subscribing to another user's narrative channel receives nothing; forged claims in payloads are ignored; shared-channel payloads carry ids only. Fraction. | §5, §6 |
| `s_privacy` | | ≤ 90 accounts per request, one request in flight, Retry-After honoured, every stored account reported once per cycle; `closed` → names erased everywhere by the run's end; `updated` → refreshed. Fraction. | §5 |

**G: Forge LLM (11 rows, ≈ 0.0082 each)**

| id | measured | anchor |
|---|---|---|
| `g_model_lifecycle` | Every `chat`/`stream` names an id `list()` reports `active` at call time, including after the 4:30 deprecation, and no sampling-rule 400s occur. Fraction of calls. | §6, reference |
| `g_tool_validation` | A forcing `tool_choice` is used. Malformed or forged arguments give an error state and nothing stored. JSON-string arguments are accepted. Fraction. | §6 |
| `g_grounding` | In the shown narrative: digits ⊆ numbers the app computed; keys ⊆ release ∩ visible; model verdict claims that differ from computed ones are not shown as verdicts; citations ⊆ visible evidence. Fraction. | §6 |
| `g_injection` | Injection scenario: no verdict change, no rendered link for model URLs, no secret in any prompt, no publish without the confirm click. Fraction. | §6 |
| `g_prompt_scope` | Person-facing prompts carry only the requester's visible content. A(wide) then B(narrow): B gets nothing that only A could see (cache scoped). Fraction. Shadow of `s_permission_leak` for the same canary. | §6 |
| `g_placement` | Every narrative job completes off the resolver path (no resolver killed at 25 s) and reaches the requester over Realtime with 0 idle invokes; 0 LLM calls at any boot. Fraction. | §6 |
| `g_rate_dosing` | During draft-all, every 60 s window holds ≤ 100 requests per installation and ≤ 500k tokens per model (stated estimator), and the batch completes. | §6 |
| `g_economy` | An identical request makes 0 new calls; every call carries `max_completion_tokens` ≤ 4,096; input ≤ 24k tokens; a rerun on unchanged evidence makes 0 calls. Fraction. | §6 |
| `g_budget_killswitch` | Per-user cap and installation budget: 0 calls after exhaustion, with `BUDGET_EXHAUSTED`. Usage and cost in `admin.getUsage` equal emulator usage × stated rates (±1 %). | §6 |
| `g_failure_states` | Each failure class shows `narrative-error` with its reason class; the app keeps working; the retry rule holds. Fraction. | §6 |
| `g_label_confirm` | AI text carries `data-ai-generated="true"`; drafts are invisible to others until an auditor publishes. | §6 |

**B: Custom UI boot (9 rows, ≈ 0.0078 each).** These are §C.6's table, one row each:

1. `b_shell_first`
2. `b_waves`
3. `b_ops`
4. `b_payload` (bytes, requests, depth and bootstrap JSON; continuous over-budget ratio, no step)
5. `b_bootstrap_scaling`
6. `b_csp_origins`
7. `b_no_rework`
8. `b_boot_429`
9. `b_boot_side_effects`

Each is measured on both surfaces and cold, with the panel on 4 issue profiles.

**U: Custom UI function (8 rows, 0.00875 each)**

| id | measured |
|---|---|
| `u_panel_content` | Verdict, reasons, visible records (kind, actor name, time, ref) and the chain-ok mark equal the oracle, for 6 issues × viewer profiles. |
| `u_workspace_overview` | Releases, posture counts, gate-ready, and chart bars proportional within 1 px, in the stated order. |
| `u_release_view` | Rows equal the visible changes; hidden count; default sort by verdict severity then key; deployed-at toggle; `aria-sort`; pages of 50 that do not skip or repeat across invocations. |
| `u_router` | Deep links render READY; issue links go through the router to `/browse/<KEY>`; back works. |
| `u_live_update` | With a release open, attestations move its posture on screen without reload (Realtime), with 0 idle invokes before the publish. Graded on the outcome only; polling scores 0 (1.0 defect H). |
| `u_export_pack` | Export JSON equals the oracle: visible records, chains verifiable, names per privacy state. |
| `u_degraded_states` | During the wall and during Jira 5xx, `[data-testid="degraded"]` with stored evidence, not blank. |
| `u_narrative_flow` | Request → pending → Realtime → draft (labelled) → publish (auditor) → visible to other auditors. |

**V: visual (4 rows, 0.0075 each):** `v_tokens_contrast`, `v_dark_surface`, `v_layout` (320 / 1280 px, no horizontal
scroll, no clipped numbers), `v_console_clean`.

**K: UI Kit admin (9 rows, ≈ 0.0067 each)**

1. `k_manifest_uikit`
2. `k_boot`
3. `k_policy_form`
4. `k_roles_table` (includes the UserPicker mapping and the sort semantics)
5. `k_integrations`
6. `k_usage_view` (rendering only; truth is graded in T and G)
7. `k_work_queue`
8. `k_migration_tab`
9. `k_quiescence` (≤ 4 reconciles per action, no `onError`, no `BridgeAPIError`)

Each is graded on the ForgeDoc, the bridge log and the resolver effects. `testId`s are used only on components that
deliver them.

**D: migration (3 rows, 0.010 each):** `d_exactly_once` (every v1 fact once, including issues with concurrent live facts
and v1 facts also present in Jira history); `d_bar_cleanup` (≤ 2 h; `ev1:*` removed only after their facts exist;
progress truthful); `d_config` (v1 policy honoured).

### F.2 Excellence slice (0.08) with STATED references (1.0's lesson: an unstated optimum rewards accident)

| id | measured | reference (stated in TABLES after calibration) |
|---|---|---|
| `x_points` | backend points for the scenario | per-phase references: backfill points per window issue, live points per fact, panel points per view, from the golden's worst of 5 seeds; score `min(1, ref/actual)` |
| `x_boot_bytes` | app bytes before READY, panel + workspace | the golden's numbers |
| `x_kvs_units` | KVS 10 KB write units | the golden's worst of 5 seeds |

The gate fraction is the proportion of {`l_lint_warnings`, `v_console_clean`, `b_csp_origins`, `t_bg_budget`,
`g_economy`, `e_exactly_once`} at 1.0.

### F.3 Composition, criticals, roots, bands

- `earned = (0.92·inner + 0.08·gate·x_mean) × crit_mult`, where `crit_mult = Π(0.6 + 0.4·s)` over unsuppressed
  criticals.
- `final = min(earned, ceiling − 0.05·(1 − earned))`. 1.0's `capped_final`, so no two models sit exactly on a cap.
- **Criticals (10):** `l_deployable`, `l_bundles_load`, `e_no_evidence_lost`, `e_no_duplicate_stamp`,
  `s_admin_bypass`, `s_permission_leak`, `s_webtrigger_forgery`, `s_secret_exposed`, `s_cross_tenant_leak`,
  `t_pool_exhausted_by_app`. Each is a fact the golden achieves on every calibration seed and schedule.
- **ROOT_BLOCKS** (attribution plus multiplier dedup; each root is priced once):

  | root | blocks |
  |---|---|
  | `l_bundles_load` | every runtime row |
  | `p_export` | E, C, D |
  | `p_pipeline` | E, C, D, U content |
  | `p_panel` | panel B, U, V rows |
  | `p_workspace` | workspace B, U, V rows |
  | `p_admin` | K |
  | `s_permission_leak` | `g_prompt_scope`, `s_auditor_scope` (same canary) |
  | `e_no_evidence_lost` | `d_exactly_once` (missing v1 facts) |
  | `t_pool_exhausted_by_app` | that hour's `t_interactive_served` |
  | a manifest section fault | its surface's criticals become `vacuous_root`, not cliffs (1.0 defect G/D4) |

- **World changes stay driveable.** When a console form fails, the probe performs the world change through the
  stated resolver (`admin.savePolicy`). The UI defect is priced in K alone and the downstream rows stay graded.
- **Vacuity.** Rows that "cannot pass on nothing" carry preconditions and score `vacuous_root` (0, no multiplier) when
  unmet. Examples: the forgery critical with no web trigger; the duplicate-stamp critical with no stamp.

**Admission bands** (`passed` = exactly 1.0, available, not vacuous):

| band | max | requires |
|---|---:|---|
| 1 deployable | 0.499 | `l_deployable`, `l_bundles_load` |
| 2 working pipeline | 0.599 | `p_pipeline`, `p_export`, `p_panel`, `p_workspace`, `p_admin` |
| 3 complete product | 0.749 | `p_capabilities` |
| 4 production grade | `max(0.749, 0.899 − 0.02·(n − 1))` | n = DEFECTS (root-deduplicated) among 40 rows: every S row, `c_chain_integrity`, `c_freshness_slo`, `e_exactly_once`, `e_verdicts`, `e_stamps`, `e_change_propagation`, `t_bg_budget`, `t_smoothing`, `t_quota_wall`, `t_near_limit`, `t_per_issue_writes`, `t_backfill_bars`, `r_retry_discipline`, `r_poison`, `r_schedules`, `g_grounding`, `g_injection`, `g_prompt_scope`, `g_rate_dosing`, `b_waves`, `b_ops`, `d_exactly_once`, `k_policy_form` |

The prompt states the four bands in words (1.0 parity test).

**Severity selftest** (wired into `--reference`; an inversion refuses the freeze):

1. A permission leak scores below a missing panel.
2. A duplicate stamp scores below a missing stamp.
3. A pool exhaustion scores below a budget overshoot that caused no wall.
4. A forked chain scores below a slow-but-intact chain (`c_chain_integrity` > `c_freshness_slo` in weight, and both
   are band-4).
5. The empty starter and a one-function app are scored (a verdict, never a refusal) at ≤ 0.05.
6. A dead bundle multiplies once.
7. A 0.95-complete evidence set multiplies by 0.6 + 0.4·0.95⁴ = 0.926 exactly.
8. Band 4 at n = 1, 4, 8, 9 and 30 gives 0.899, 0.839, 0.759, 0.749, 0.749.
9. Every DESIGN partial-credit statement (for example "polling scores 0") is asserted against the code (1.0 defect H).

### F.4 One-defect mutants (≥ 1 per check; ≈ 60, authored from this table, never from scorer output)

Notation: `Tn` is the n-th row of tier T in its §F.1 table. For example, `E10` is `e_no_duplicate_stamp` and `S5` is
`s_permission_leak`.

| area | mutants |
|---|---|
| concurrency | `m_global_serial` (C2), `m_rmw_chainhead` (C1, C3), `m_dedupe_jobid` (E3), `m_query_for_dedupe` (E3, under query lag) |
| ambiguous failure and per-issue writes | `m_retry_post_blind` (E10 critical), `m_comment_per_deployment` (T6), `m_no_stamp_pacing` (T6) |
| Tier 1 dosing | `m_no_budget` (T1, T11), `m_burst_at_00` (T2), `m_ignore_r` (T4), `m_retry_quota_early` (T3), `m_numeric_retry_after_only` (R2, gateway variant), `m_sleep_in_function` (R3), `m_full_rescan_hourly` (T1, X1), `m_no_export_throttle` (T9) |
| robustness | `m_no_checkpoint` (R1), `m_throw_on_404` (R4, E7), `m_no_dead_letter` (R4), `m_sched_no_lease` (R5), `m_install_no_retry` (R6), `m_drop_v1_queue` (R7, E9 critical), `m_unawaited_push` (R8, E9) |
| security | `m_module_cache_by_issue` (S critical tenant), `m_admin_display_condition_only` (S1 critical), `m_role_from_payload`, `m_idor_payload_id`, `m_asapp_panel` (S5 critical), `m_mypermissions_for_issue` (S5, security level), `m_sign_parsed_json`, `m_header_case`, `m_no_ts_window`, `m_get_then_set_nonce` (S10), `m_secret_kvs_set`, `m_secret_in_config` (critical), `m_innerhtml_pipeline` (S14), `m_rovo_input_account`, `m_realtime_client_claims`, `m_report_promise_all`, `m_no_erase_closed`, `m_perm_cache_forever` (S7) |
| LLM | `m_hardcoded_model`, `m_temperature_top_p`, `m_trust_tool_args`, `m_show_model_numbers`, `m_model_verdict_shown`, `m_asapp_prompt`, `m_llm_in_resolver`, `m_no_rpm_pacing`, `m_cache_no_scope`, `m_no_killswitch`, `m_llm_retry_storm`, `m_publish_without_confirm` |
| boot and UI | `m_await_before_render`, `m_waterfall`, `m_dev_react_bundle`, `m_admin_code_in_panel`, `m_reinvoke_on_nav`, `m_poll_instead`, `m_no_degraded_state` |
| UI Kit | `m_userpicker_object`, `m_display_date_sortkey`, `m_no_handlesubmit`, `m_textfield_email_register` |
| migration | `m_migrate_no_issue_lock` (D1), `m_v1_keys_deleted_early` (D2) |

Each mutant has an expectation `{loses: [...], critical: bool, max_final}`. For platform-semantics mutants (races,
KVS codes), the mutant must ALSO misbehave on real Forge (fidelity layer L2a, §I), or it measures the emulator.

---------------------------------------------------------------------------------------------------------------------

## G. Difficulty argument (for GPT-6.1 Sol and Opus 5.5)

### G.1 What 1.0 showed

*"On everything Forge 1.0 states, the frontier has no headroom."* Sol scored 0.9769 and Opus 0.9767. Their entire
residual was an unstated economy row (frontier-behaviour §0, §4). The mechanism: *"The contract did the hard thinking.
Every semantic trap that both models handled traces to a contract sentence that Haiku paraphrased in its own design
notes."* Testing found only wiring: *"All six defect classes Haiku found by testing were platform wiring, each announced
by an explicit dev-tool error string."* The budget never bound: *"They used 72 and 50 of 150 calls"*.

2.0 therefore:

- states invariants and numbers, not the fault list;
- makes the decisive defects produce wrong STATE, not error strings (a fork, a duplicate comment, a leaked canary,
  an over-budget hour);
- puts the difficulty in trade-offs between stated constraints.

### G.2 The requirements Sol and Opus most plausibly fail

Each row is a STATED requirement, with the behaviour 1.0's transcripts showed and the observable.

| # | stated requirement | why plausible (frontier-behaviour quotes) | observable | rows at risk |
|---|---|---|---|---|
| 1 | **G2 integrity + G3 freshness under a release-train burst** | Haiku: *"I'm accepting the eventual consistency tradeoff here since concurrent writes both read fresh state anyway"*. Pareto serialized *"the whole site through one consumer (`limit: 1`), which is correct but has a throughput ceiling."* A racy append forks or overwrites the chain under the reads-first schedule. Global serialization misses 120 s on ~420 attestations. Only per-issue keys or conditional CES transactions pass both. | chain checks; first-seen times | `c_chain_integrity`, `c_freshness_slo`, `c_posture_race`, `e_exactly_once` (≈ 0.03–0.06 inner, 1–3 band-4 defects) |
| 2 | **Exactly-once stamps under ambiguous 5xx and per-issue write limits** | Pareto's comment: *"Never retry a successful POST; the UI coalesces a double click before invoking this resolver."* Haiku: *"rather than adding backend-side deduplication"*. Here the writes are backend-initiated, so no UI can coalesce. A 503-after-commit plus a naive retry gives two comments, which is the critical. One comment per deployment on a 60-deployment umbrella issue breaks 20 writes per 2 s. | Jira comment store; gateway | `e_no_duplicate_stamp` (C), `t_per_issue_writes`, `e_stamps` |
| 3 | **Tier 1: budget + smoothing + completion bar** | *"both reconcile designs rescan every issue updated since the earliest active sprint started… every hour"*. Haiku *"had designed the fix and dropped it"* (*"My plan: persist a `lastRunAt` watermark…"*). Full rescans or N+1 reads either bust the 4,000/h background budget or miss the 3 h bar. Front-loaded backfills fail smoothing. | gateway ledger | `t_bg_budget`, `t_smoothing`, `t_backfill_bars`, `x_points` |
| 4 | **The quota wall from other customers; the gateway 429 without Retry-After** | *"both sleep through Retry-After inside the invocation"* (Haiku: *"which works fine since consumer invocations can run up to 900 seconds"*, *"while never setting `timeoutSeconds`"*). Haiku parsed *"numeric seconds only"*. A wall lasting tens of minutes cannot be slept through; the gateway variant has no Retry-After; `r` appears only past 80 %. | gateway; `admin.getUsage`; degraded responses | `t_quota_wall`, `t_near_limit`, `r_no_long_waits`, `r_retry_discipline`, `t_usage_truth` |
| 5 | **World changes converge** | *"Haiku caches the Sprint field and each board's estimation field forever"*; *"Pareto's consumer throws on a 404, so a deleted issue is redelivered for 24 hours."* Policy edits, deletions, permission loss and budget changes all happen mid-run. | checkpoints | `e_change_propagation`, `r_poison`, `s_permission_ttl` |
| 6 | **Live v1 migration, exactly once** | *"Neither app versions its storage… both treat KVS as a fresh store."* v1 approvals duplicate Jira history; v1 deployments exist nowhere else; v1 events sit in `cp-events`. | export | `d_exactly_once`, `e_no_evidence_lost` (C), `r_deploy_midflight` |
| 7 | **Security that produces no error strings** | Haiku *"never exercised the restricted viewer"*: *"Permission filtering was not tested with a user who cannot browse some issues"*. In 2.0 a leak is silent: the auditor rule, `mypermissions` used where security levels apply (sec#14), module caches without `cloudId` across sites whose ids overlap, per-visibility LLM caches. | canary scans | `s_permission_leak` (C), `s_cross_tenant_leak` (C), `g_prompt_scope`, `s_auditor_scope` |
| 8 | **LLM dosing and economy** | 1.0 tested one explain call per click. Draft-all over 36 releases with tool loops exceeds 100 RPM unless paced; a cache keyed per tenant serves a wide-visibility draft to a narrow viewer. | LLM log | `g_rate_dosing`, `g_economy`, `g_prompt_scope` |
| 9 | **Issue panel boot with a server-side check under a points budget** | The panel must check permission server-side (extension ids are not trusted), but every check costs points on 1,000 views per hour. Caching beyond 300 s fails the TTL; no caching inflates points; checking in the browser fails IDOR. | waves, ops, gateway | `b_ops`, `s_permission_ttl`, `x_points`, `s_idor` |
| 10 | **Privacy reporting without a burst** | The helper fires every 90-account batch with `Promise.all` (sec#31); the mock refuses a second concurrent request. | privacy mock | `s_privacy` |

### G.3 Predicted scores

| entrant | predicted final | reasoning |
|---|---|---|
| golden | 1.000 | It is the gate. |
| alt app | ≥ 0.95 | It is the gate. |
| **GPT-6.1 Sol** | **0.45–0.75** (central ≈ 0.65) | Build: Sol likely completes every surface (it finished 1.0 in 13 min). Losses: rows 1, 3, 4, 8 almost certainly in part, with inner ≈ 0.78–0.85. Band-4 defects ≈ 6–12, so the cap is 0.749–0.799. Probability of ≥ 1 critical (duplicate stamp, cross-tenant cache or a leak) ≈ 40 %; one critical gives × 0.6, so 0.45–0.50. |
| **Opus 5.5** | 0.45–0.78 | Opus took twice Sol's time on 1.0 (1,587.7 s against 791.5 s), suggesting more deliberation. It is likely stronger on rows 6–7 and similar on 1–4. |
| Haiku 5.5 | 0.30–0.55 | 1.0 transcript: *"accepting the eventual consistency tradeoff"*, in-function sleeps, numeric-only Retry-After, untested restricted viewer. |
| mid models (Sonnet-class, Qwen Flash) | 0.25–0.50 | Bands 2–3 plus partial E, T and S. |
| weak or wiring-limited models | 0.00–0.30 | Band 1–2. 1.0's zero class (manifest shapes) still applies. |

Confidence is MEDIUM-LOW: there are no Sol or Opus trees on this machine, so their 1.0 internals are inferred from Haiku
and Pareto (frontier-behaviour §6). The phase-4 pre-build difficulty proof (fresh Opus-class planners against the
hidden check list) is the check before anything is built.

### G.4 Why this is engineering judgment, not trivia or volume

Every hard row sits on a trade-off between two STATED constraints. Satisfying one naively breaks the other:

| constraint A | constraint B | naive pass of A breaks B | judgment that passes both |
|---|---|---|---|
| chain integrity | 120 s freshness | global `limit:1` | partition by issue, or conditional transactions |
| exactly-once stamps | per-issue write limits | one comment per deployment | coalesced comments plus a stamp ledger and check-before-retry |
| background budget | backfill bar | sleep or full rescans | bulk endpoints, watermarks, chunked continuation |
| security TTL | points | no cache, or a long cache | a 300 s scoped cache |
| LLM economy | permission scope | a tenant-wide cache | a visibility-scoped key |
| boot budget | features | one mega-bundle | code-split admin code; one bootstrap invoke; bridge reads |
| migration exactly-once | live flow | migrate-then-switch | a shared per-issue lock and identity-based dedupe |

None of it is API trivia: each primitive is in the typings or the reference pack. None of it is volume: Pareto showed
that writing 11 files lint-clean the first time is easy (frontier-behaviour §5.3). The budget is set so that it does
not bind for a strong model (§J.5), so failures measure design, not speed.

### G.5 If Sol still cruises

The acceptance criterion (NOW.md phase 7): Sol loses real points on stated engineering requirements, meaning
**final ≤ 0.85, ≥ 5 band-4 defects, and ≥ 0.08 of inner lost in C, T, R or S**, each loss traced to a contract
sentence. If Sol clears that, the hardening order is:

1. raise the train rate (SLO pressure);
2. add a second wall in the backfill hour;
3. tighten the smoothing window;
4. add the optional `jira:jqlFunction` (1,000-value cap, user-agnostic precomputations).

All of these are stated changes; nothing becomes hidden.

---------------------------------------------------------------------------------------------------------------------

## H. Fairness argument: how a capable model can know every graded thing

1. **Guarantees and numbers are in the contract.** Every check's anchor is a contract sentence (§F anchors). The
   1.0-style check↔contract map test fails the build on an unanchored check or an unchecked hook.
2. **Platform behaviour is documented and shipped offline.**
   - The reference pack holds verbatim Atlassian excerpts (only BRIEF-`confirmed` claims, corrected wording, URL and
     date on each) on async events, product events, scheduled triggers, web triggers, invocation limits, runtime (warm
     reuse, unawaited work, memory), KVS/CES, Realtime, LLM, Jira rate limiting, permissions, security
     responsibilities, privacy, Custom UI CSP and the bridge, and UI Kit components.
   - The typings, manifest schema and OpenAPI are pinned.
   - Where Atlassian is silent or self-contradictory, the contract picks and states (BRIEF §9.1's 31 pins, §9.2's knobs).
3. **The dev world exhibits every fault CLASS** at least once over its timeline: duplicates, permutation, loss, late
   delivery, sender retries, ambiguous 5xx, burst 429, near-limit and wall, the gateway 429, schedule duplicate and
   skip, warm reuse across its two installations, query lag, cursor invalidation, the install 403 window, deploy
   mid-flight, LLM failure classes, and Realtime and KVS limits. They come unlabelled, as in production, and the dev
   ledger, call log and `forge-dev clock` make them observable. A model that tests finds them; a model that reads
   finds them in the pack.
4. **Nothing hidden is graded.** Fault timing and intensity are private. Every fault CLASS is documented (pack) or
   stated (§11). No score row rests on an unmeasured emulator guess (§I rule).
5. **Grading is outcome-only.** Any design meeting the guarantee passes: no "must use transactions", no keys-in-
   storage schema, no required channel names. The grader reads state through the stated `audit.exportChains`, the
   surfaces and the Rovo action.
6. **Proof of sufficiency.**
   - The golden is built from the public text only, by an agent that never sees the scorer (1.0 §13.3).
   - The **independent alt app** (different storage design, e.g. per-issue concurrency keys instead of conditional
     transactions; different endpoints) must score ≥ 0.95 with zero `harness_missing`.
   - **Phase 4:** fresh Opus-class planners write plans from the public text alone. A check that NO plan could
     anticipate from the text is rewritten as text before freeze.
7. **No deception in the dev kit.** The dev site has no scripted-answer printout and no fault menu, but it also has no
   behaviour the scoring site lacks. Its classes are a subset, at lower intensity.

---------------------------------------------------------------------------------------------------------------------

## I. Fidelity plan

**The rule:** a score row may depend only on

- (a) a live wolfaenpak receipt, from the **same probe source run live and in the emulator**, with any difference
  failing the kit test (real-forge-fidelity §4, L1);
- (b) a doc quote that no live run contradicts; or
- (c) a STATED benchmark rule.

A harness choice that is none of these may not decide a row unless the row accepts every plausible reading. Real Forge
is never a score input (L0–L3 architecture). Rate-limit tests are never run on a cloud tenant ("Do not perform rate
limit testing against Atlassian cloud tenants", tiers#48).

| # | emulated behaviour | live probe (wolfaenpak throwaway app; same code in the emulator) | status | if unmeasured, the design… |
|---|---|---|---|---|
| I-1 | KVS/CES codes, atomicity, limits | the 24-case probe | **done**: 13/24 matched; fix the 11 (409 `KEY_CONFLICT`, 400/422 codes…) | — |
| I-2 | CES conditional set or check on a MISSING entity (N5) | add cases | planned (P1) | the golden must not rely on it before the receipt (§C.1 uses entity `set` + `FAIL_IF_EXISTS` to create heads) |
| I-3 | KVS/CES query: default and max limit, cursor reuse across invocations, lag after write | `/kvs-query` probe | planned | the 2 s lag and cursor-validity rules are STATED (§11); grading uses outcomes |
| I-4 | queue retry schedule, `retryReason` values, `InvocationError` clamp, 4 KB `retryData` | consumer throws on attempts 0–4 | planned (~40 min idle) | the redelivery schedule is STATED |
| I-5 | delivery concurrency, `concurrency{key,limit}` exactness, duplicates, ordering | 50 pushes with and without keys | planned | the in-flight cap (≤ 32) is STATED; key semantics must match the receipt |
| I-6 | push limits (50, 500/min, 200 KB, delay) | push 51, 201 KB, delay | planned | — |
| I-7 | timeouts (consumer `timeoutSeconds`, scheduled, web trigger 55 s, resolver 25 s in the browser) | logging-until-killed functions | planned | the kill semantics are STATED (partial writes persist) |
| I-8 | static web trigger outputs, the request shape (arrays, case, raw body), web trigger 429 headers, `webTrigger.getUrl` | outputs probe; header-echo probe | partly done (request shape, 424) | the HMAC scheme is app-level and STATED; the platform `hmacSharedSecret` is not graded |
| I-9 | `avi:jira:deleted:issue` payload; self-generated events with `ignoreSelf` | delete and comment from the app | planned | |
| I-10 | `avi:forge:upgraded:app` on a major upgrade; old events delivered to new code | deploy v1 → push → deploy v2 major → `install --upgrade` | planned | deploy-mid-flight is a documented class (CHANGE-2526) |
| I-11 | install race (asApp 403 after install) | install and immediately call asApp | best effort (eventually consistent) | the 10 min window is STATED |
| I-12 | asApp visibility of issue-security levels | create a level the app user is not in, read asApp | planned (settles BRIEF §2.4 item 2) | worst case STATED as an emulator assumption whatever the probe shows |
| I-13 | `permissions/check`, `mypermissions` (ADMINISTER), asUser 404 on unbrowsable issues, asUser `search/jql` filtering | bed users with different permissions | planned | — |
| I-14 | issue properties PUT, ADF comment POST, `changelog/bulkfetch`, `issue/bulkfetch` with fields (1000), `search/jql` paging | shape probes | partly (1.0 read side) | — |
| I-15 | `@forge/llm`: validation errors, `list()` statuses, stream chunking of tool calls, refusal `finish_reason`, the 429 RPM shape | `/llm` probe (cents of tokens; confirm LLM is enabled for the dev space) | planned | every error shape is STATED; rows accept any `ForgeLlmAPIError` status |
| I-16 | Realtime: `publishGlobal` from a consumer, token claims, subscribe-only and publish-only tokens, `errors[]` at 50 ops/s | consumer publishes; Custom UI subscribes (browser leg) | planned (quiet window) | pairing STATED verbatim (1.0 defect D) |
| I-17 | Personal data reporting: test ids Active and Closed, 429 shape | report the two documented test ids | planned (safe, documented ids) | concurrency refusal is a STATED benchmark rule |
| I-18 | UI Kit host callbacks (Form `onSubmit`, Tabs `onChange`, UserPicker `onChange`, Select `isMulti`, DynamicTable sort) | probe admin page; capture ForgeDoc and callback args | planned (browser) | frozen as golden fixtures; unmodelled components → `held` |
| I-19 | Custom UI served form, CSP, bridge injection | 1.0 receipts + a re-check | done in part | — |
| I-20 | lint server-rule pack for every 2.0 feature | ~40 corpus manifests through real `forge lint`, ~6.5 s each | planned | only measured rules are charged; documented-but-unenforced constraints (v04–v06) are not charged |
| I-21 | multi-entry resources: bridge injected into named entries | deploy a 2-entry resource; open both | planned | until it passes, the contract allows but does not suggest multi-entry |
| I-22 | Jira rate-limit header GRAMMAR (passive) | read `Beta-`/`RateLimit-Policy` headers on normal responses; never approach a limit | planned | quota and burst NUMBERS are STATED benchmark values |
| I-23 | `@forge/api` `route` path-injection throw | done (source read, sec#S22) | done | — |
| I-24 | scheduled-trigger duplicates and skips; warm reuse across tenants; unawaited suspension | not inducible live | — | documented classes (pack) or STATED (§11); outcome-graded |

**Kit lint (L0).** The CLI client half is already byte-identical. Add:

- the pinned flag value;
- the server-rule pack (6 rules measured so far, extended by I-20);
- a CLI-equal file walk (`.mjs`/`.cjs`, UI Kit resource directories, `.gitignore`);
- `ConfigFile`-style interpolation;
- provenance per finding (`client` | `server-measured <date>` | `docs-only (not charged)`);
- a weekly drift job, run online by us only.

**Two more freeze-time layers.**

- **L2a golden gate:** the golden and alt deploy, install and pass a smoke test on wolfaenpak. Platform-semantics
  mutants (the race, the wrong KVS code) must misbehave on real Forge too.
- **L2b audit lane:** our published baseline runs get a deploy-only check that compares accept or refuse with the
  scorer's deploy verdict.

**Every schema-valid form is supported** (the consumer `resolver:` arm, both web trigger response types), or the kit
lint refuses it with the real CLI's message (1.0 defect C).

---------------------------------------------------------------------------------------------------------------------

## J. Build plan, effort, risks, sizes, budget, cost, scoring time, packaging

### J.1 Work packages (one owner per file; 2.0 lives in its own tree, `evals/swarm-bench/forge2/`, integration D1)

| WP | owns | reuse from 1.0 (machinery §1) | effort (agent-days) | confidence |
|---|---|---|---|---|
| WP1 emulator core | runner (in-child virtual-time agent, `Math.random` seeding, warm multi-tenant worker pool, timer freeze), the proxy-gated deterministic scheduler, queue scheduler (keys across queues, ≤ 32 in flight), web trigger ingress (static/dynamic, raw body, array headers, `getUrl` GraphQL), lifecycle and upgrade events, the KVS fidelity fixes (I-1..I-3) plus 10 KB units and limits, a Realtime v2 broker, an LLM v2 responder (scenario by release, stream, parallel tools, RPM/TPM windows, `list()` statuses, latency), the privacy mock | runtime/proxy/kvs ≈ 60–80 % | 8–10 | MEDIUM (scheduler MEDIUM-LOW) |
| WP2 site | multi-installation mock Jira; the points gateway (pool, phantom draw, cost table, buckets, per-issue writes, header emission, gateway variant); the mutable world timeline (policy, deletions, permissions, account closure, scope); comments and properties with ambiguous 5xx; the permission model (security levels, ADMINISTER, `permissions/check`, `mypermissions`); a scalable fixture generator (fix N3); evidence diet (page summaries, NDJSON spill) | site/jql/rest ≈ 50–70 % | 7–9 | MEDIUM |
| WP3 UI Kit host | the bridge ops, ForgeDoc snapshots, the allowlisted render to accessible HTML, the ADS comparator (en-US), host UserPicker, flags outside the app region, fixture capture (I-18) | the 1.0 spike `prove-uikit.cjs` | 5–6 | MEDIUM |
| WP4 scorer, probe, oracle | `score_forge2.py` (registry, composition, bands, criticals, roots, selftest), the probe phases, the security, LLM and boot batteries (wave protocol), the indexed oracle, `forge2_controls.py` | composition/selftest ≈ 50 % | 8–10 | MEDIUM |
| WP5 golden, alt, mutants | the reference app (≈ 9k LOC), an independent alt (≈ 8k LOC), ≈ 60 mutants with expectations | mutant generator | 8–10 | MEDIUM |
| WP6 public text, pack, integration | contract, tables, prompt, starter with v1 appendix; the reference pack (≈ 70 KB from verified quotes); the FORGE20 tier, per-tier budget, desktop and site per-era support (integration §3–§7) | 1.0 patterns | 4–5 | HIGH |
| fidelity | probes I-2..I-22 on wolfaenpak, quiet windows for browser legs | the 1.0 probe app | 3–4 | HIGH (mechanism) |

**Total ≈ 43–54 agent-days.** Parallel wall time ≈ 3–4 weeks. Order: WP1 + WP2 + fidelity start day 0 → WP5's golden
builds against forge-dev from day ~5 (public text only) → WP4 scores the golden → controls → calibration → freeze →
phase 7 pilot.

### J.2 Risks (ranked by correctness risk, not effort) and the cut list decided in advance

1. **The deterministic scheduler (MEDIUM-LOW).** If it slips, C rows must not run on nondeterministic physical
   concurrency. The fallback is a seeded per-request "yield points" scheduler at the proxy (simpler, still
   deterministic), with only reads-first and serial schedules. C rows never ship ungated.
2. **Emulator fidelity of CES conditions and queue concurrency** (I-2, I-5). The golden waits for the receipts. Rows
   depend on them only after the receipts exist.
3. **Calibration feasibility.** The golden must meet every bar on both worlds and all schedules, while the naive
   mutants (`m_no_budget`, `m_global_serial`, `m_full_rescan_hourly`) must fail them. If the margins are thin, adjust
   scale and budgets (stated numbers) before freeze, never thresholds after.
4. **UI Kit host (MEDIUM).** The real renderer is closed. Mitigations: captured fixtures, broad component coverage,
   `held` for unmodelled components.
5. **Scoring time and evidence size at scale.** The evidence diet and the indexed oracle are prerequisites (machinery
   §4.4); the site must run outside the probe process (N8).
6. **Public input creep.** ≈ 36 KB is already over 1.0's 25 KB. The tables carry numbers; any addition must displace
   something.
7. **Licence.** Shipping verbatim Atlassian doc excerpts in the pack (like 1.0's OpenAPI question, R4) is an owner
   decision.

**Cut order if the build or the frontier calibration says "too hard"** (never the core E, C, T or S):

1. Rovo actions and MCP (−3 rows);
2. privacy reporting (−1 row and 1 critical surface);
3. the UI Kit Work-queue and Migration tabs (−2 rows);
4. brownfield migration (D tier and R7, −4 rows; the starter becomes a fresh install).

### J.3 Reference app size

| part | LOC |
|---|---|
| backend: governor, chain, verdicts, consumers, web trigger, migration, privacy, LLM pipeline, resolvers | ≈ 4,800 |
| Custom UI (panel + workspace, router, charts, timeline, export) | ≈ 2,800 |
| UI Kit admin | ≈ 1,300 |
| manifest | ≈ 260 |
| build scripts | ≈ 150 |
| **total** | **≈ 9,000–9,500 LOC in 50–60 files** |

For comparison, 1.0's golden was ≈ 1,940 LOC of src + UI.

### J.4 Packaging size added

- **Desktop payload:** ≈ +3 MB (`forge2/public`, starter, kit code, site, the ≈ 70 KB reference pack, scorer files).
  The OpenAPI copies move or duplicate (7.1 MB) depending on whether 1.0's trees still ship (integration D2).
- **Kit cache** (materialised once by `npm ci` from the lock): app-modules gain `@forge/react` 12.3.0. Its closure is
  143.6 MB unpacked by registry metadata, ≤ 306 MB on disk, with partial overlap with `@forge/bridge`'s dependencies,
  so **≈ +130–280 MB**. A real `npm ci` + `du` must run before freeze (BRIEF §5.0). Lint-modules are unchanged (≈ 172
  MB).
- No SQL engine (0), no Confluence mock (0). Chromium is already bundled.

### J.5 Call budget recommendation: **300** (a per-tier field; integration D5 keeps SB7.2 at 150)

| work | calls |
|---|---|
| reading (contract, starter, typings, schema, OpenAPI, pack greps) | 20–35 |
| writing ≈ 9k LOC at 300–600 LOC per call | 20–35 |
| lint and wiring loop | 10–20 |
| dev scenarios (virtual-time backfill, train, wall, LLM, admin, boots) | 50–90 |
| **a strong model** | **≈ 110–180** |

Reasons for 300:

- The challenge must come from stated engineering, not the budget. A tighter budget *"would mostly punish exploration
  style… a speed test, not a depth test"* (frontier-behaviour §5.3).
- Testing concurrency and dosing needs several dev scenarios.
- Sol's extra cost from the headroom is small (below), and weak models' cost stays ≈ $1.

If the owner wants budget pressure as a secondary signal, 225 is the minimum I would defend. Below that, Sol is graded
on triage.

### J.6 Per-run model cost (assumptions stated; replace with the pilot's measurement)

| model (price per M in / out / cache-read) | calls | prompt tokens (cached share) | output | per run |
|---|---|---|---|---|
| GPT-6.1 Sol ($2 / $10 / $0.10) | ≈ 150–220 | 18–40 M (≈ 95 %) → ≈ $1.7–3.8 cached + $1.8–4.0 uncached | 150–450k → $1.5–4.5 | **≈ $5–12** (1.0: $0.72 in 13 min, about 50–70 calls) |
| cheap model, GPT-6 Luna-class ($0.10 / $0.50 / $0.01) | 300 (uses the whole budget) | ≈ 50–60 M → ≈ $0.5–0.6 cached + $0.25–0.3 uncached | ≈ 0.6–0.9 M → $0.3–0.45 | **≈ $1.0–1.3** |

Scoring costs no model calls.

### J.7 Scoring time (estimate, unmeasured; run serially on the scoring host)

| step | per world |
|---|---|
| build and lint | ≈ 0.5 min |
| backend phases A–F, ≈ 1.5–2.5k invocations on a warm worker pool | ≈ 3–6 min |
| phase B replays (2 extra schedules) | ≈ 2–4 min |
| browser (panel × 4 profiles × 2 themes, workspace flows) | ≈ 3–4 min |
| UI Kit host flows | ≈ 1 min |
| **one world** | **≈ 10–15 min** |

Two worlds give ≈ **20–30 min**, plus ≈ 1–5 min of oracle and composition, so **20–35 min** per tree (1.0: 6–7 min for
3 sites). A pathological app (N+1 everywhere) is bounded: proxy calls cost ≈ 1 ms each (measured on 1.0), so 50k calls
add under 1 min. Machine load stretches wall time only; virtual time, waves and schedules keep scores load-free.

---------------------------------------------------------------------------------------------------------------------

## K. Open decisions for the owner

1. **Call budget:** 300 as a per-tier field (recommended), or 225, or keep the shared 150.
2. **Public input ≈ 36 KB** plus an on-demand ≈ 70 KB reference pack. Accept, or force cuts to ≤ 30 KB (drop §9 Rovo
   and §10 v1 first).
3. **Brownfield migration:** keep (recommended; it is frontier-behaviour item 5, "a kind they have not been tested on")
   or cut (−4 rows, simpler starter).
4. **Two scoring worlds × three phase-B schedules** (recommended, 20–35 min), or three worlds (≈ 30–50 min).
5. **Critical multiplier:** keep 1.0's 0.6 + 0.4·s over 10 observed-consequence criticals (one critical puts Sol near
   0.45), or soften to 0.7 + 0.3·s to spread the frontier more. Recommendation: keep 0.6, since each critical is a
   genuine customer harm and the selftest guards inversions.
6. **RoA required** (recommended; drives the static trigger, no egress and in-app dosing) or optional.
7. **One more latest module:** add `jira:jqlFunction` ("issue in complianceViolations()": 1,000-value cap,
   user-agnostic precomputations, staleness after verdict changes). It is strongly emulatable. Not recommended unless
   the pilot shows Sol cruising.
8. **Excluded GA/Preview features** (Forge SQL, Confluence, `dashboards:widget`, `rovo:skill`, app-managed
   permissions): confirm the exclusions.
9. **Shipping verbatim Atlassian doc excerpts** in the kit (licence; like 1.0's OpenAPI question).
10. **Stated scale numbers** (12,000 / 2,500 / 400 issues; train of 420; 4,000/h budget; 3 h bar) are proposals until
    the golden and the naive mutants are calibrated. Approve "calibrate, then freeze into text", never "freeze, then
    tune thresholds".
11. **Forge 1.0's fate** (NOW.md open question): fix and re-score before the 2.0 flip, or freeze with a note. Integration
    §7 step 0 requires the decision before any 2.0 registration.

---------------------------------------------------------------------------------------------------------------------

### Appendix: mandate checklist → where it is graded

| # | mandate item | stated in | graded by |
|---|---|---|---|
| 1 | latest Forge modules combined | §2, B | `l_platform_current`, every surface row; LLM GA 07-30, Realtime tokens, UI Kit 2, `rovo:mcp` Preview 10-01, bulkfetch 1000 (08-28), `rateLimitProperties` (08-13), FRGE-1923 headers |
| 2 | big, tough app | §0–§11 | 101 weighted rows; ≈ 9k LOC reference |
| 3 | complex backend resolvers | §1, §3, C.3 | E, C, U, `audit.exportChains` |
| 4 | Forge security concepts | §5 | S (17 rows, 5 criticals), `l_platform_current` (RoA) |
| 5 | stability and robustness | §3, §11 | R, C, D, `e_no_evidence_lost` |
| 6 | Custom UI boot speed | §7 | B (9 rows) |
| 7 | front-facing Custom UI (pretty) | §7 | U, V, screenshots and video |
| 8 | UI Kit admin panel | §8 | K (9 rows) |
| 9 | a ton of calls, dosed to Tier 1, robust under bursts | §4 | T (11 rows), `x_points`, `c_freshness_slo` |
| 10 | good Forge LLM usage | §6 | G (11 rows) |
| 11 | judicious checking (1.0 §17.8) | DESIGN F, I | observed-consequence criticals, roots priced once, continuous rows, harness evidence never app evidence, flags outside the app, every schema form supported, contract = emulator text, worst-of-world/schedule, one-defect mutants, alt ≥ 0.95, selftest |
| 12 | proof that Opus/Sol are challenged | G | phase-4 plans against hidden checks, then the phase-7 Sol pilot with the §G.5 criterion |
