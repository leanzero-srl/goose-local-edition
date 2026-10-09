# Forge 2.0 panel design: "Loadline" (angle: SYSTEMS TORTURE)

Independent panel entry, 2026-10-09. Built only on the confirmed facts of `research/BRIEF.md` and its topic notes
(tags `tiers#`, `rob#`, `sec#`, `llm#`, `uikit#`, `boot#`, `[B]` point at their verification tables), the measured
live receipts in `research/understand/real-forge-fidelity.md`, and the 1.0 lessons in `forge/DESIGN.md` §17.8.
Nothing here was run. Numbers marked **CTF** are "calibrate on the golden, then freeze as a number in the contract".

**One paragraph.** The app is a workload and capacity radar for Jira Cloud. Its arithmetic is deliberately simple
(sum of remaining estimates per assignee against capacity), because Forge 1.0 proved the frontier transcribes stated
semantics perfectly. All the difficulty sits in OPERATING that arithmetic correctly: four installations sharing one
Tier 1 pool of 65,000 points per hour, a 10,000-issue bulk edit delivered concurrently and out of order to triggers
that race on shared state, invocations killed at their virtual deadlines, a quota wall that lasts longer than any
retry the platform allows, Jira writes that commit and still answer 503, a world that deletes projects and moves
issues mid-run, and a live brownfield migration of every customer's Loadline 1 data while events keep flowing. The
contract states guarantees, envelopes and invariants, never the injection plan. The grader is a deterministic
discrete-event simulator on a virtual clock, so concurrency is real (interleaved at every I/O) yet every score is
byte-reproducible and independent of machine load.

---------------------------------------------------------------------------------------------------------------------

## A. Product pitch

**Loadline — who is overloaded, right now, across every project.** Engineering managers and team leads on large Jira
Cloud sites cannot see real load: work is spread over many projects, estimates change every hour, people take leave,
and the only capacity view Jira offers is per plan. Loadline keeps, for every person, the open remaining estimate
assigned to them across the whole site, compares it with their capacity minus absences fed by the company's HR
system, and flags overload and work-in-progress excess. Teams see a live heatmap; a lead drills into a person and
sees the issues they are allowed to see; Loadline's AI explains the picture and proposes concrete reassignments that
the lead confirms with one click. Admins run it from a native Jira admin panel: teams and capacities, roles,
policies, the HR integration and its signing secret, AI budgets, and an Operations view of sync health and API
consumption. JQL users get `issue in overloadedWork()`, a dashboard gets a "my load" widget, and Rovo can answer
"who on Platform is overloaded?".

Why a customer pays: it is a real gap (cross-project capacity), it integrates the HR system, and it saves leads hours
per week. Why it is hard to build well, and therefore why the benchmark is not benchmaxxed: a workload app on a
30,000-issue site sees thousands of updates per minute during a bulk edit, must stay exact, must not burn the
vendor's shared Tier 1 pool (one tenant's sync can lock every other customer out for an hour, tiers#27), and ships
as version 2 of an app that is already installed with data in an old schema. The arithmetic is not the product; the
correctness of the arithmetic under production conditions is.

---------------------------------------------------------------------------------------------------------------------

## B. Modules

Status column cites the brief. "Hard" is what the benchmark actually exercises.

| # | module (key) | status | why Loadline needs it | what makes it hard |
|---|---|---|---|---|
| 1 | `jira:globalPage` `loadline-board`, Custom UI | GA; one per app [B] | the front-facing board (pretty, live) | count-based boot budget whose bootstrap must not scale with team size (forces materialised snapshots, which brings back concurrency); live updates through Realtime under a 50 ops/s cap; user-context drill-down through bridge `requestJira` (exempt from points, tiers#24,25; headers visible since FRGE-1923); paused and migrating states |
| 2 | `jira:adminPage` `loadline-admin`, `render: native` (UI Kit 2, `@forge/react` 12.3.0), `useAsConfig` | GA (uikit#18,#20) | the admin panel (mandate) | UI Kit host semantics (`useForm` blur validation, `Form.onSubmit` gets no data, UserPicker stores an object, host-owned sort by cell key, Tabs), subpages are Custom UI only so navigation is Tabs; who may open the page is undocumented, so every resolver authorises itself |
| 3 | `dashboards:widget` `my-load` + `edit`, Custom UI | GA 2026-09-22 (CHANGE-3453; `edit` required [B]) | "my load" on dashboards | edit API persistence, second instance, live update, viewer-scoped data |
| 4 | `trigger` on `avi:jira:created:issue`, `avi:jira:updated:issue`, `avi:jira:deleted:issue`, `avi:jira:deleted:project`, `avi:forge:installed:app`, `avi:forge:upgraded:app` | GA | event-driven ingestion and lifecycle | at-least-once, up to 3 min late, unordered, concurrent (rob#33); only top-level deletes (rob#36); `selfGenerated` (rob#35); install before permissions (rob#37); upgraded only on a major version [B]; 25 s runtime |
| 5 | `consumer` (queues `jobs`, `recompute`, `apply`, `llm`, and v1's `load-events`) | GA | batch work, continuation, migration, AI, apply | v2+ retry semantics, `InvocationError` by return value (rob#24), concurrency keys per installation across queues (rob#28), 50 events/push, 500 events/min, 200 KB/push, 100 KB per event for long consumers, cyclic 1,000 (rob#15-18), `timeoutSeconds` ≤ 900 |
| 6 | `scheduledTrigger` `reconcile` (`hour`) and `tick` (`fiveMinute`) | GA | anti-entropy heal, continuation kicker | ≤ 5 and one `fiveMinute` (rob#38); duplicates, overlap, skips (rob#40); a throw is not retried (rob#39) |
| 7 | `webtrigger` `hr-feed`, **static** response | GA; only static triggers are RoA-eligible (sec#18) | HR absence feed | app-implemented HMAC + timestamp + delivery-id replay protection (platform HMAC is undocumented, sec#17), LWW with tombstones under out-of-order at-least-once delivery, bursts, secret rotation |
| 8 | `jira:jqlFunction` `overloadedWork` | GA; contract captured [B] | JQL and filters | precomputations are not per user, ≤ 1,000 right-hand values, 25 s, and must be kept fresh by the app through the computation API |
| 9 | `llm` (`@forge/llm` 1.0.7) | GA (llm#1) | rebalance proposals, weekly digest | no structured outputs, forced tool, sampling-parameter traps (llm#16,17), 100 RPM per installation, 500k TPM per model (llm#20,21), no Retry-After through the SDK (llm#37), 5-min inference only in consumers ≥ 300 s (llm#23), developer-billed |
| 10 | `action` `get-workload` + `rovo:agent` + `rovo:skill` + `rovo:mcp` | action/agent GA; skill Preview (CHANGE-3499); mcp Preview (CHANGE-3400/3495) | Rovo access; `get-workload` is also the graded read contract | identity from context only (sec#26), inputs untrusted, storage-only answers |
| 11 | Forge Realtime (`@forge/realtime` 1.0.1) | GA; publish-only/subscribe-only tokens (CHANGE-3326) | live board, private AI delivery | 50 ops/s per installation including UI subscriptions (rob#63); claims derived server-side (llm newer fact 5); `subscribe()` scoped to the module context by default [B] |
| 12 | KVS + Custom Entity Store + secrets | GA (rob#43-59) | all state | conditions only in CES transactions (rob#49), no `keyPolicy` in batches or transactions (rob#46), `transact().set` TTL in the 4th argument (rob#50), query eventually consistent (rob#43), unstable cursors (rob#57), 10 KB units, 1 MB/s per key, 32-bit integers |

Required platform posture: **Runs-on-Atlassian eligible** (no egress, no remotes, static web triggers only, sec#20),
`nodejs22.x` or `nodejs24.x` (20 is now a server-side lint ERROR, real-forge-fidelity §2.3 v08).

**Deliberately excluded** (each a stated harness deviation where relevant): Forge SQL (needs a MySQL-compatible
engine, dialect fidelity unverified, RFC-148 images unverified — the kit lint refuses `sql` with a stated message);
Object Store (Preview, needs a pre-signed host); Confluence (a second mock product is volume, not systems
difficulty, and is blocked at the scope layer today, machinery N2); `apiRoute`, app-managed permissions, workflow
rules (Preview, unmeasured contracts); `jira:customField` (when Jira invokes the value function is unmeasured; open
decision K7); multi-entry resources (bridge injection into named entries never observed, BRIEF §1.1).

---------------------------------------------------------------------------------------------------------------------

## C. Architecture

This is the **reference (golden) architecture**. The contract states outcomes, so any design reaching them passes;
§F names the alternative designs the independent alt app must use to prove that.

### C.1 The numbers (stated in contract §1; the only domain arithmetic)

For every person P of an installation, from Jira's current state plus Loadline's own data:
- **open issue**: an issue assigned to P whose status category is not `done` and that no lead has excluded.
- **loadHours** = Σ `timeestimate` seconds of P's open issues ÷ 3600, summed in seconds then rounded once, half away
  from zero, to one decimal.
- **wip** = number of P's open issues in status category `indeterminate`.
- **capacityHours** (from Loadline's team editor, one value per person); **availableHours** = max(0, capacity − Σ
  hours of P's active absences); `null` when P is in no team.
- **utilizationPct** = 100 × loadHours ÷ availableHours, one decimal; `null` when availableHours is 0 or null.
- **overloaded** = utilizationPct > policy `overloadPct` OR wip > policy `wipLimit`.
- Inactive people: `active: false`, all numbers 0/`null`, their issues count for nobody. Unassigned: nobody.

### C.2 Data model (reference)

Custom entities (≤ 20 entities, ≤ 7 indexes, one range attribute per index — server-enforced, real-forge-fidelity §2.3):

| entity (key) | attributes | indexes | written by | concurrency rule |
|---|---|---|---|---|
| `contribution` (issueId) | accountId, projectId, seconds (int), cat, excluded (bool), ver (string: zero-padded changelog id or `updated` epoch-ms; changelog ids exceed 2^31 on the scoring site, so not `integer`) | `by-assignee` [accountId] / [projectId]; `by-project` [projectId] / [issueId] | triggers, catch-up, migration, purge | CES transaction `set` with condition "absent OR ver < new" (`CONDITIONAL_CHECK_FAILED` = an older or equal version lost the race, which is success) |
| `person` (accountId) | capacitySeconds, active, displayName, teams (any), ver | — (team membership lives in `team`) | admin, reconcile | CAS on `ver` |
| `team` (slug) | name, members (any), leads (any), ver | — | admin, migration | CAS on `ver` (two admins editing) |
| `absence` (absenceId) | accountId, seconds, seq (int), deleted (bool) | `by-person` [accountId] / [absenceId] | webtrigger, migration | condition "absent OR seq < new"; deletes leave tombstones |
| `snapshot` (teamSlug) | per-member numbers (any), basedOnVer, at | — | `recompute` consumer only (concurrency key per team) | single writer per team |
| `job` (jobId) | kind, phase, watermark (key range, never a cursor), lease {owner, until}, ver, progress | `by-kind` | job steps | lease via conditional `set`; watermark advanced in the same transaction as the step's last write |
| `deadletter` (id) | kind, ref, reason, attempts, firstAt, state | `by-state` | job steps | FAIL_IF_EXISTS on create |
| `ledger` (`<hourIso>:<class>:<shard>`) | points (int), calls (int), quota429 (int) | — | the Jira gateway wrapper | 16 shards per (hour, class); conditional increment with retry |
| `llmjob` (jobId) | team, requester, inputHash, model, status, proposal (any), usage | `by-requester` | `llm` consumer | FAIL_IF_EXISTS on create (dedupe) |

Plain KVS: `cfg` (overloadPct, wipLimit, admins), `pause` (pausedUntil, reason), `precomp:<id>` (JQL precomputation
ids), `pending:<team>` (debounce marker, FAIL_IF_EXISTS), `nonce:<deliveryId>` (FAIL_IF_EXISTS, TTL 2 days, read with
`EXPIRE_TIME` metadata because expired values stay readable up to 48 h, rob#47), `apply:<jobId>` (FAIL_IF_EXISTS).
Secrets: `kvs.setSecret('hook:current')`, `hook:previous` + rotation deadline (sec#22).

### C.3 Async topology (reference)

```
 Jira product events ──► trigger issue-events (25 s, up to 50 concurrent per installation)
   created/updated/deleted      │ event-carried state (fields probed live, §I P10) → version-guarded CES upsert
                                │ then  set pending:<team> FAIL_IF_EXISTS → on success push recompute(team, delay 20 s)
                                ▼
 trigger project-deleted ──► push jobs{purge, projectId}                 consumer recompute (concurrency team:<slug>, 1)
 trigger installed ─────────► create job backfill (FAIL_IF_EXISTS)        │ delete pending:<team> FIRST, then query
 trigger upgraded ──────────► create job migrate  (FAIL_IF_EXISTS)        │ by-assignee, write snapshot, one Realtime
                                                                          │ notice per team per recompute (≤ 1 / 20 s)
 consumer jobs (900 s, concurrency job:<id>, 1)                           ▼
   step = lease → chunk (≤ 2,000 issues, budget-checked) → write → advance watermark in-txn → re-enqueue next step
   kinds: migrate, backfill, catchup, purge, rescan(person), digest-fanout; remaining time checked before each chunk
   (`invocationRemainingTimeInMillis`, rob#6); quota pause → re-enqueue with delayInSeconds = min(900, until − now)

 consumer load-events (v1's queue, kept)  body {issueId} → read issue → version-guarded upsert
 consumer apply (300 s, concurrency apply:<jobId>, 1)  idempotent multi-step move (C.5)
 consumer llm (600 s, concurrency llm, 4) + installation RPM/TPM token bucket in CES (conditional updates)
 scheduled reconcile (hour, 900 s): lease (FAIL_IF_EXISTS + expireTime) → catch-up from watermark → anti-entropy:
   per person approximate-count vs stored count, targeted rescans; users' active flags via /user/bulk;
   dead-letter retries; JQL precomputation refresh; never full rescans
 scheduled tick (fiveMinute): resume jobs whose lease expired, lift the pause after reset, expire secret rotation
 webtrigger hr-feed (55 s, static): verify → nonce → seq-guarded absence write → pending:<team> → outputKey
```

Why each choice: a 1:1 trigger→queue push design cannot meet the burst bar, because 10,000 events at 500 events per
minute is 20 minutes of pushing (rob#15) — the app must either process in the trigger or batch many issues per queue
event; the debounce marker is deleted before the recompute reads, so a change racing the recompute schedules another
one (no lost update without a lock); the watermark is a key range, because cursors are only valid inside the
invocation that produced them (stated, rob#57); continuation steps of 2,000 issues keep any job under the cyclic
1,000-push limit (24,000 issues = 12 steps).

### C.4 Resolver catalogue (reference; keys are the golden's, the contract does not name resolver keys)

Every resolver returns a value and never throws; refusals return `{"error":"forbidden"}`; identity is
`context.accountId` only (sec#1,2); `accountType` other than `licensed` is refused on every module.

| key | inputs | authorization | Jira calls | failure handling |
|---|---|---|---|---|
| `board.bootstrap` | `{teamSlug?}` | licensed user | 0 (storage ≤ 6 ops) | `migrating` / `paused` states; returns a subscribe-only Realtime token for `team:<slug>` |
| `board.team` | `{teamSlug}` | licensed user | 0 | unknown team → `{error}` |
| `board.person` | `{accountId}` | licensed user | 0 | returns open-issue count + excluded ids; the list itself is read in the browser with bridge `requestJira` as the viewer (0 points, permission-correct by construction); hidden = count − visible |
| `board.exclude` | `{issueId, excluded}` | lead of a team containing the assignee, or admin; BROWSE on that exact issue via `permissions/check` (verdict cached ≤ 5 virtual min, per installation × account × issue) | ≤ 1 | `route` with the id (path manipulation throws, [B] safeUrl) |
| `ai.suggest` | `{teamSlug}` | lead of the team or admin; AI enabled; budget left | 0 | enqueue `llm` job (id = hash(snapshot ver, lead, model)); cache hit returns the stored proposal; returns a subscribe-only token for `ai:<accountId>` |
| `ai.apply` | `{jobId, issueIds}` | the requesting lead; every id must be in that proposal (IDOR) | 0 | `apply:<jobId>` FAIL_IF_EXISTS → a double click or second tab joins the same apply |
| `widget.me` / `widget.team` | `{}` / `{teamSlug}` | licensed user | 0 | `needs-config` |
| `admin.*` (teams, roles, policy, hook, ai, ops, deadletters, digest) | per tab | **admin**: granted admin, or Jira ADMINISTER verified live (`mypermissions?permissions=ADMINISTER` asUser, BRIEF §2.1) cached ≤ 5 virtual min | 0–1 | CAS on `ver` for concurrent admins; last-admin and self-revoke refused |
| action `get-workload` | `{accountIds?, teamSlug?}` | licensed user; identity from context | 0 | schema-valid `{error}` entries for unknown ids |
| jqlFunction `overloadedWork` | `{precomputationId, clause}` | none (no user) | 0 | returns `{jql:"assignee in (…) AND statusCategory != Done"}` — an assignee predicate, so it stays ≤ 120 values and does not change when issues move |

### C.5 Rate-tier dosing (the "ton of calls" made affordable)

One gateway wrapper around `api.asApp()/asUser().requestJira` used by every code path:
1. **Self-accounting.** Charges each response by the published cost table (counts returned objects), classifies the
   call `background` or `user-initiated` by the invocation's origin (carried in queue bodies), adds to sharded
   `ledger` entries. The Operations tab reads the shards.
2. **Background budget.** Before every background call: points spent this virtual hour + this call's estimate ≤ the
   stated per-installation budget (8,000, CTF), else defer to the next hour (re-enqueue, delay ≤ 900 s, repeated).
3. **Near-limit reserve.** Any response whose `RateLimit` carries `r` (pool > 80 % used, tiers#17) sets
   `pause{until: reset, reason: reserve}` for background work; user-initiated work continues.
4. **Quota wall.** Any 429 whose reason is `jira-quota-global-based`, unknown, or absent (gateway variant without
   Retry-After, tiers#15) sets `pause{until: reset}` for everything; background re-enqueues; resolvers that need
   Jira return `paused`. Bridge reads keep working (exempt from points, stated).
5. **Burst / per-issue.** `jira-burst-based` slows that endpoint only (≥ Retry-After; an in-invocation wait ≤ 5 s is
   allowed, longer defers); `jira-per-issue-on-write` delays writes to that issue only (tiers#20).
6. **Concurrency.** ≤ 10 backend product requests in flight per installation: Jira-calling work runs only in
   consumers whose concurrency key caps them; triggers make no Jira calls.
7. **Retries.** Idempotent requests only, ≥ Retry-After, ≤ 4 attempts; a non-idempotent POST with an ambiguous
   outcome (5xx, timeout) is never retried before a read proves it did not land.
8. **Bulk first.** `search/jql` with named `fields`, `issue/bulkfetch` with fields (≤ 1,000 per call, tiers#29),
   `user/bulk`, `permissions/check` for up to 1,000 issues per call (sec#13), `search/approximate-count` for
   anti-entropy (1 point).

### C.6 LLM features (reference)

1. **Rebalance proposal** (a lead, per team). `llm` consumer (timeout 600 s; injected latency 40–140 virtual s, so a
   resolver path times out). Model from `list()` at call time, `active` only, re-listed after a 403
   `FORGE_LLMS_MODEL_FORBIDDEN`; no `temperature`/`top_p` on the models that reject them (llm#16,17);
   `max_completion_tokens` always; forced tool `propose_moves {summary: string, moves: [{issueKey, toAccountId,
   reason}]}`. Prompt: the app's computed team numbers plus the overloaded members' open issues the LEAD may browse
   (asApp read + `permissions/check` for the lead on those exact issues; no asUser in consumers), untrusted text
   delimited. Every move is validated (visible to the lead, assigned to an overloaded member, target in the team,
   target stays ≤ overloadPct after the move, not excluded); invalid moves are dropped with a reason. The summary is
   shown only if it has no digits, otherwise the app's own sentence; links never rendered. Delivered on `ai:<lead>`
   (token claims signed server-side, client token subscribe-only).
2. **Apply** (confirmed moves). `apply` consumer, asApp after a `permissions/check` binding the lead to ASSIGN_ISSUES
   and BROWSE on the same issue ids (sec#7): per move, record intent (FAIL_IF_EXISTS) → re-read assignee (skip as
   `stale` if a human changed it since the proposal) → PUT assignee (idempotent) → comment `[loadline:<jobId>:<issue>]`:
   if a previous attempt may have posted, read comments for the marker first; on 5xx or timeout never re-POST before
   that read → mark done. A crash after any step resumes correctly on redelivery.
3. **Weekly digest** (admin button). One narrative per person with load + one per team (144 on the large scoring
   site): paced by a CES token bucket at ≤ 100 requests and ≤ 500,000 estimated tokens per model per 60 virtual s,
   cached by (installation, input hash, model, viewer scope), charged against monthly credit budgets set in the
   admin panel (usage × stated credit rates, llm#30-32).

### C.7 Front-facing Custom UI (`loadline-board`) and its boot budget

Layout: team pills; a heatmap grid of person cards (avatar, load bar against available, utilization ring coloured by
token bands, WIP chips, overload badge); a drawer with the person's visible open issues and a hidden count; for
leads an AI panel (proposal cards, checkboxes, Apply, per-move outcome chips, AI label); banners for `migrating`
(progress %) and `paused` (reset time). Light and dark through `view.theme.enable()` and `--ds-*` tokens.

Boot (stated, counted, never timed; BRIEF §4.3 protocol): the bootstrap `invoke` is issued at module top level
before React renders; the shell renders at wave 0; READY after ≤ 1 backend wave with ≤ 2 backend-bound ops; ≤ 220 KB
gzip-9 app-origin bytes and ≤ 6 app-origin requests before READY (CTF); initiator depth ≤ 3; the bootstrap resolver
makes 0 Jira calls and ≤ 6 storage operations in ≤ 3 rounds on every team size; the AI panel is a lazy chunk loaded
on demand; Realtime subscribes after READY; under a 429 on the bootstrap invoke the shell stays and exactly one retry
fires at or after `rateLimitReset` (boot#16).

### C.8 UI Kit admin panel (`loadline-admin`)

Tabs (controlled) — **Teams**: DynamicTable `teams-table` (slug, name, members, utilization with a numeric cell key),
`team-new`, a Modal form (Textfield `team-slug`, `team-name`, UserPicker labelled "Members" mapped to `.id`,
per-member capacity Textfields, Leads picker), validation via `useForm` + `handleSubmit`, Save as LoadingButton;
**Roles**: `roles-table`, grant via UserPicker, revoke buttons, last-admin refusal message; **Integrations**: webhook
URL (`webTrigger.getUrl`), `hook-configured` yes/no, password Textfield `hook-secret`, rotation status; **AI**:
enabled Toggle, monthly credit budgets, credits used; **Operations**: `ops-points-bg`, `ops-points-ui`,
`ops-quota-429`, `ops-paused-until`, `ops-backlog`, `ops-migration`, `dead-letters` table with retry and discard,
"Run digest". Every action is a resolver that re-checks admin server-side (C.4).

### C.9 Web trigger `hr-feed` (stated scheme, contract §6)

Headers (looked up case-insensitively; values arrive as arrays): `x-loadline-timestamp` (unix seconds),
`x-loadline-delivery` (id), `x-loadline-signature: sha256=<hex>`; signed bytes `<timestamp>.<delivery>.<raw body>`;
HMAC-SHA256 with the installation's secret, compared in constant time; accepted only within ±300 virtual s; a
delivery id acts at most once for 24 h; during a rotation's 10-minute grace both secrets verify. Body
`{"type":"absence.upsert","absenceId","accountId","hours","seq"}` or `{"type":"absence.delete","absenceId","seq"}`;
per absenceId only a higher `seq` applies; a delete leaves a tombstone. Static outputs: `accepted` 202, `duplicate`
200, `unauthorized` 401, `invalid` 400. No absence, capacity or number changes before verification succeeds.

### C.10 Brownfield: Loadline 1 → 2 (stated in contract §7)

The workspace starts as **Loadline 1** (source, manifest, built UI). v1 schema (shipped in `v1/SCHEMA.md` and readable
in its source): `meta {schema:1, syncedThrough, cursor}`; `cfg {overloadPct, wipLimit, admins, teams:[{slug, name,
members:[{accountId, capacityHours}], leads}]}`; per person `load:<accountId> {total, issues:{<issueId>:{s, cat, u}}}`;
`ovr:<issueId> {excluded, by, at}`; `abs:<absenceId> {accountId, hours, seq, deleted}`; the webhook secret in a
PLAIN `kvs.set('hook-secret')`; queue `load-events` (consumer `load-consumer`, body `{issueId}`).

Stated guarantee: v1 data is exact for every issue whose `updated` ≤ `syncedThrough`; v1 knows nothing after it;
v1's queue holds events for some later changes; issues deleted after it still appear in v1 blobs. The scoring harness
deploys the entrant's v2 over v1's live data (KVS and pending queue events kept, `avi:forge:upgraded:app` delivered
because v2 is a major version: new scopes and the `llm` module, [B]).

Reference migration: job `migrate` (FAIL_IF_EXISTS) → (1) secret to `setSecret`, plaintext deleted; (2) cfg → teams,
people, policies, admins; (3) absences with tombstones; (4) overrides; (5) `load:*` blobs in key order (resume from
the last key, never a stored cursor), each contribution written only if absent or older (live events may already
have written newer rows); (6) catch-up `updated >= syncedThrough − 2m` (JQL minute precision); (7) deletion
reconcile via approximate counts; (8) flip `meta.schema = 2`, recompute every snapshot; (9) delete v1 keys. v1's
queue keeps its consumer. Until the flip every read reports `state: "migrating"` with progress — never a stale
number. Migration + catch-up costs about 1,000 points on the large site; a full rebuild costs ≥ 24,000 points and
cannot meet the 60-minute deadline inside an 8,000-points-per-hour budget — that gap is intended.

---------------------------------------------------------------------------------------------------------------------

## D. The world

### D.1 The world engine (what grades it): a deterministic discrete-event simulator

The axis that makes this benchmark hard (real concurrency) is exactly what Forge 1.0's harness could not do: its
queue drained strictly serially (machinery §1, N11) and its clock ran on wall time. 2.0 replaces it with one engine,
used identically by the dev kit and the scorer:

- **Execution environments.** Each environment is a worker thread (own V8 isolate, own module registry and globals,
  `resourceLimits.maxOldGenerationSizeMb = memoryMB`, rob#8) that loads Atlassian's pinned runtime wrapper and the
  app bundle, and serves invocations one at a time. Environments are **reused warm across invocations and across
  installations** by a seeded policy (tenant-isolation tests, sec#29); a new one starts cold when all are busy.
  Fallback if the wrapper refuses to run in a worker: one child process per environment with the same agent (a build
  choice proven by the conformance suite, never a runtime fallback, gate 1).
- **In-environment agent.** Installed before the wrapper: virtualises `Date`, `performance.now`, `process.hrtime`,
  `setTimeout`/`setInterval`/`timers/promises`; seeds `Math.random` and `crypto.randomUUID` per invocation; routes
  every I/O (`__forge_fetch__`, `__requestAtlassianAsApp`, LLM, Realtime GraphQL, egress) to the scheduler; supplies
  `global.__forge_runtime__` with a `lambdaContext` closure for `getRemainingTimeInMillis` (rob#7); reports **parked**
  when the microtask queue is drained and `process.getActiveResourcesInfo()` shows only scheduler-held handles.
- **Scheduler (conservative DES).** Every pending I/O has a virtual completion time (stated latency × a seeded factor
  in [0.7, 1.3]); every timer a virtual deadline; every delivery a ready time. The scheduler waits until all busy
  environments are parked, then fires the earliest event (ties by seeded RNG); a request is **linearised at its
  completion** against the mock services, so races between invocations are real and every interleaving is
  reproducible. CPU time is free (lenient, stated). Kills happen when virtual time passes an invocation's deadline;
  writes already completed persist (the kill semantics BRIEF §3.4 says the contract must state).
- **Unawaited work.** At handler return the agent freezes pending timers and promises; requests they issue are held
  and released at the start of the environment's NEXT invocation — possibly another installation's — or never if
  the environment is recycled. This is the Node.js runtime warning made executable (rob#12, BRIEF §3.1).
- **Services behind the scheduler** (separate process, holds the fixtures and the oracle; the app's environments are
  fenced by `sandbox-exec` and cannot read them, 1.0 §6.1): mock Jira with the rate-limit gateway, KVS/CES aligned to
  the live probe codes, queues (push limits, retries, retention, concurrency keys, jobs), product events, scheduled
  triggers, web-trigger ingress (raw body, multi-value headers), lifecycle and upgrade, Realtime broker, scripted LLM.
- **Load independence.** Nothing graded reads the wall clock. Real-time watchdogs exist only to protect the harness:
  an environment that does not park within 30 real seconds is killed and the verdict is HELD and re-scored; it
  becomes app evidence only if it recurs on the re-score (1.0 `held` mechanism). Golden's longest non-yielding slice
  is expected in tens of milliseconds, a margin of ~1,000×.
- **Determinism proof (freeze gate).** Golden, alt and every mutant scored three times per world produce
  byte-identical observation logs.

### D.2 Scoring worlds and scale

Two worlds per scored tree: **World A** (the run's `fixture_seed`) and **World B** (a derived seed, ~⅓ scale). Each
correctness row takes its WORST world (1.0 §17.7 F3); each economy row the mean.

| installation | role | World A | World B |
|---|---|---|---|
| I1 Northwind | large, was on Loadline 1 | 24,000 issues, 10 projects, 120 people, 24 teams | 8,000 / 4 / 60 / 12 |
| I2 Bluefin | mid, was on Loadline 1; world changes | 6,000 / 4 / 40 / 8 | — |
| I3 Kestrel | small, was on Loadline 1; the interactive probe site | 900 / 2 / 12 / 3 | 900 / 2 / 12 / 3 |
| I4 Juniper | installs v2 fresh mid-run | 3,000 / 3 / 20 / admin creates 3 | 1,500 / 2 / 12 / 2 |

Per installation: three viewer personas (admin, lead, member); issue-security levels hiding ~3 % of issues from
members (canary summaries); one project the member cannot browse; issue keys repeat across installations (`OPS-12`
exists on three sites); changelog ids above 2^31 on I1; tenant-specific canary strings in summaries, team names and
secrets.

### D.3 Timeline (World A, virtual UTC, Monday 2026-11-02). The contract states the ENVELOPE, never these times.

| virtual time | what happens |
|---|---|
| 08:10 | v2 deployed over Loadline 1 on I1–I3; pending v1 queue events start delivering; `upgraded` event within 3 min |
| 08:10–09:10 | trickle of ~1 update/s across sites (create, edit, reassign, re-estimate, resolve); HR trickle |
| 08:25 | during-migration reads (action + board) |
| 08:40–08:42 | HR import burst on I1: 3,000 signed deliveries (duplicates, out-of-order seqs, replays, a forged/stale/tampered/case-varied set) |
| 08:50 | admin rotates I2's webhook secret through the admin panel; deliveries signed with old and new secret follow |
| 09:05 | I4 installed; asApp answers 403 for up to 20 virtual min; admin creates 3 teams through the admin panel at 09:08 |
| 09:10 | **C1** migration deadline: I1–I3 exact; **U1** board on I3 (member, lead × light, dark; cold boot counted; drill-down; one boot under an invoke 429) |
| 09:20–09:23 | **burst** on I1: 10,000 updates on ≈ 7,000 issues (bulk reassignment + re-estimates; concurrent same-issue updates, duplicates, reordering, missing events, 600+ issues landing on single people). World B: 3,000. On I2: a project deleted (~600 issues), 40 issues moved between projects, 25 deleted, 2 people deactivated, 300 resolved |
| 09:38 | **C2** burst bar (15 min after the burst's last change) |
| 09:45 | **U2** board open on I1 as a lead while a 200-update mini-burst hits the team; Realtime ops counted |
| 09:50 | **U3** AI flow on I1: suggest → proposal via Realtime → apply 6 moves (2 comment POSTs commit-then-503, 1 environment crash after the assignee PUT, 1 issue reassigned by a human after the proposal, 1 per-issue 429); **U6** widget on I3 |
| 09:55 | digest batch on I1 (144 LLM jobs) via the admin panel button |
| 10:00–12:00 | reconcile hours; scheduled duplicates, an overlap and a skipped tick are injected |
| ~10:33 | pool passes 80 % (other installations' load): `r` appears |
| ~10:41–11:00 | **quota wall**: every backend product call 429 (standard and gateway variants), Retry-After up to ~1,140 s; **U4** at 10:45 on I3 (board, drill-down via bridge, AI paused state) |
| 11:30 | **C3** post-wall: everything that was deferred has landed |
| 12:00–12:20 | heal of missing events, deletes, moves, deactivations; at 12:00 a re-estimate wave flips three I1 people's overload status; JQL freshness probes (two users) at 12:05 and 12:15 |
| 12:30 | **C4** final exactness (critical) |
| 12:35 | **U5** admin panel on I3 (tabs, team edit, validation, double submit, sort, Operations values, dead letters, roles, secret form) |
| 12:45 | **security battery** on I2/I3 (direct resolver calls as every principal, webhook attacks, Rovo forgery, anonymous sessions) |
| 13:00–14:00 | idle hour → **C5** (nothing new: zero storage writes) |

Throughout: four installations share the env pool (warm cross-tenant reuse), other installations draw ≤ 30,000
points per normal hour from the pool, other apps draw from the same burst buckets, and a seeded set of async and
product-event invocations crash after their k-th I/O (platform fault: redelivered, earlier writes kept).

### D.4 The world's guarantees as the contract states them (§3 of the contract, verbatim intent)

- "Four installations of your app share one Tier 1 pool of 65,000 points per virtual UTC hour. Three were running
  Loadline 1 when v2 is deployed; a fourth installs v2 during the scenario. A site holds up to 30,000 issues, 150
  people and 30 teams. Issue keys repeat across sites."
- "People keep working: issues are created, edited, reassigned, re-estimated, moved between projects (id stays, key
  changes), resolved and deleted; projects are deleted; people are deactivated; admins edit teams and policies; the
  HR system sends absences, sometimes thousands in two minutes. One bulk operation can change up to 10,000 issues
  within 3 minutes."
- "Delivery is Forge's: product events and async events arrive at least once, possibly late, in any order, possibly
  concurrently with each other and with any other work; some product events are never delivered. Scheduled triggers
  can fire twice, overlap a run still in progress, or skip a tick. An invocation can end at any point without
  returning — at its timeout or by a platform fault; what it already wrote stays written and its event is
  redelivered."
- "Jira can commit a write and still answer with an error status, or not answer before the request times out."
- "Other installations of your app use the same pool: they draw up to 30,000 points (CTF, J.3) in a normal hour,
  and at some point they exhaust it until the next reset. Other apps share every burst bucket."
- "A newly installed site can answer 403 to asApp calls for up to 20 minutes after the installed event."
- "Forge LLM's model list changes during the scenario; `list()` is the truth at call time."

These are classes and envelopes, each a documented platform behaviour or a stated benchmark choice. The contract
never says how many duplicates, which issues, which minute, or which endpoint.

### D.5 Disjoint probe populations (how one defect stays one defect)

The fixture generator assigns every graded person to exactly ONE mechanism population, and each row grades only its
population: **plain** (burst throughput), **hot** (3 people receiving 600+ issues each in the burst: lost-update
detector), **same-issue** (4 people whose issues receive three concurrent conflicting updates), **duplicate-only**,
**reorder-only**, **missing-event** (healed by reconcile), **world-change** (moves, deletes, project delete,
deactivation), **override** (v1 exclusions: migration rows), **capacity** (v1 capacities: migration rows),
**absence** (webhook rows), **stale-overwrite** (updated live during migration). Read-contract fields are split by
owner too: `loadHours`/`wip` are graded by pipeline rows; `availableHours`/`utilizationPct` by migration and webhook
rows; `overloaded` only by the JQL row (with migration as its ROOT_BLOCKS root). This is how "each root cause priced
once" is enforced by construction rather than by attribution guesses.

### D.6 Dev world vs scoring worlds

The dev world uses the same engine and generator with a different seed and smaller scale: Dev-1 (2,000 issues, v1
data), Dev-2 (400, v1 data), Dev-3 (300, fresh install). `forge-dev world run [--until <phase>]` plays a compressed
timeline in which **every fault class occurs at least once** (kit test `dev_world_exhibits_every_class`): upgrade
with pending v1 events, a 600-update burst with concurrent same-issue updates, a project delete, moves, deletes, a
quota wall with both 429 variants, `r` appearance, a long Retry-After, commit-then-503 writes, crashes, scheduled
duplicates/overlaps/skips, install 403s, LLM scripts, a webhook stream with forgeries and replays.

The dev site behaves like production, not like a linter: a race produces wrong numbers, not an error. The dev kit
exposes the SAME ledgers the grader reads (`forge-dev world report`: points per installation per hour by class,
quota 429s, max in-flight requests, push rejections, KVS 429s and units, Realtime ops per second, LLM windows,
invocation kills and crashes, I/O after return) — instruments, never verdicts — and `forge-dev jira GET <path>`
(admin read of the dev site, not charged) so a careful model can compute the truth itself. There is no oracle
command. This deliberately separates "write it and let the tools complain" from engineering that verifies
(frontier-behaviour §5.2 item 7).

---------------------------------------------------------------------------------------------------------------------

## E. Contract outline (public input budget)

| file | content | est. size |
|---|---|---|
| `spec-build-forge2.md` (prompt) | mission, done criteria, model-call budget sentence, score bands in words | 3.0 KB |
| `LOADLINE-CONTRACT.md` | §0–§13 below | 23 KB |
| `LIMITS.md` | tables only: cost table, gateway headers, platform limits with documented values and error shapes, time model | 6 KB |
| `STARTER.md` | workspace = Loadline 1, dev kit commands, dev world envelope, "never call the site directly" | 4 KB |
| `report.schema.json` | JSON Schema of `get-workload` (validated by `forge-dev schema check`) | 2 KB |
| `v1/SCHEMA.md` | Loadline 1 storage and queue format | 2 KB |
| **total** | | **≈ 40 KB** (1.0: 18.9 KB) |

Contract sections and their key sentences (abridged; the real text is denser):

- **§0 Product and grading.** "The harness installs v2 over Loadline 1 on several seeded Jira sites and grades it by
  running it on a virtual clock: product events, queues, schedules, web-trigger deliveries, resolver calls, Rovo,
  and every surface in a browser. Correctness is graded whenever the world has been quiet for 2 virtual minutes;
  efficiency by counts, never by time." (1 KB)
- **§1 The numbers.** C.1 verbatim, with rounding, null rules, inactive and unassigned. (1.5 KB)
- **§2 Modules.** The table of §B (keys and roles = WHAT), RoA eligibility, runtime, "every resolver returns a value
  and never throws", "identity comes from the invocation context only", read contract `get-workload` (schema file).
  (2 KB)
- **§3 The world.** D.4. (2.5 KB)
- **§4 Invariants.** "Exact: whenever the world has been quiet for 2 minutes, every number Loadline reports equals
  §1 computed from Jira and Loadline's own data." "Throughput: after a bulk change of up to 10,000 issues, numbers
  are exact within 15 minutes of its last change." "Convergence: a change whose event never arrives is reflected
  by the end of the first hourly reconciliation that starts after it, at most 75 minutes later, not counting time
  the pool is exhausted." "A newly installed site is exact within 60 minutes of its installed event." "Exactly once:
  every Jira write Loadline intends happens once — one comment per applied move — whatever the retries,
  redeliveries, crashes, double clicks or second tabs." "Idle: an hour with nothing new writes nothing to storage
  and spends at most the idle allowance LIMITS.md states (CTF)." "Nothing after return: no Jira, storage, queue, LLM
  or Realtime request is made after a handler has returned." (2 KB)
- **§5 Rate limits and dosing.** "The pool and every Jira limit behave as LIMITS.md states: a hard wall at 65,000
  points until the top of the next virtual UTC hour; costs per the cost table; burst buckets per site × endpoint ×
  method; per-issue write windows." "Background work (anything not descending from a person's resolver or action
  call) of one installation spends at most 8,000 points per virtual hour (CTF)." "When a response carries `r`,
  background work makes no product call until the reset; user-initiated work continues." "After a quota 429 (reason
  `jira-quota-global-based`, any unknown reason, or none) no backend product call is made by that installation
  before the reset." "At most 10 backend product requests in flight per installation." "Retry only idempotent
  requests, never before Retry-After (or the reset when absent), at most 4 attempts; never re-send a non-idempotent
  write whose outcome is unknown before reading whether it landed." "An in-invocation wait may not exceed 5 seconds."
  "Scheduled work spreads: no 5-minute window holds more scheduled-origin points than a quarter of the hourly
  background budget." "The Operations tab reports this hour's points by class within ±2 % of the gateway's ledger."
  (2.5 KB)
- **§6 Security.** Roles (admin = granted admin or Jira ADMINISTER verified live; lead per team; member), who may do
  what (table), refusal shape `{"error":"forbidden"}`, "resolvers are callable directly by anyone who can load the
  module", "the admin page is admin-only in every resolver", permission rule "a person sees only issues they can
  browse; checks bind to the exact issue ids acted on; a cached verdict may be used for at most 5 virtual minutes",
  asApp visibility (worst case: everything, including security levels), `context.extension` ids are not proof of
  permission, the web-trigger scheme C.9 in full, secrets ("stored with `kvs.setSecret`; read back only as
  configured yes/no"), "warm processes are reused across installations and the harness exercises it", rendering
  (text only; links only to `/browse/<KEY>`), Realtime ("claims derived server-side; client tokens subscribe-only").
  (3 KB)
- **§7 Loadline 1 → 2.** C.10's schema pointer and guarantee, "the harness deploys v2 over Loadline 1's live data;
  queued v1 events are delivered to v2", "app-owned data (teams, capacities, policies, admins, absences with
  tombstones, exclusions) survives exactly", "the plaintext secret is moved and erased", "until migration completes
  every read reports either exact numbers or `state: migrating` with progress — never another number", "exact
  within 60 minutes of the upgrade", "afterwards no v1 keys remain". (2 KB)
- **§8 Board.** DOM hooks (root, team options with `aria-pressed`, one `[data-testid=person][data-account-id]` per
  active member ordered by utilization desc, nulls last, ties by display name; `[data-metric]` load, available,
  utilization (`85.5%` or `—`), wip; `data-overloaded`; drawer, issue rows, `hidden-count`; AI panel hooks;
  `paused`, `migrating`), READY/SHELL definitions and every boot budget, the 429 rule, live updates "without reload
  or polling". (2.5 KB)
- **§9 Widget.** `needs-config`, edit through the dashboards edit API, second instance, own numbers. (0.7 KB)
- **§10 Admin panel.** Tabs, testIds (only on components that deliver them, uikit#31), UserPicker by label,
  sort "by the number", form behaviour ("invalid input shows the stated message and calls nothing; a valid Save
  calls once, even if activated twice"), Operations field meanings. (2 KB)
- **§11 AI.** Tool schemas, "model output is untrusted", validation rules, "no write without the lead's confirm",
  digest batch, budgets and credit rates, the 429 rule ("after an LLM 429 the installation makes no LLM request for
  60 virtual seconds"), failure UI states, AI label. (2 KB)
- **§12 Rovo.** `get-workload` inputs/outputs (schema file), skill frontmatter rules, agent, mcp. (1 KB)
- **§13 Time and harness model.** "All time is virtual; CPU is free; each request takes the latency LIMITS.md states
  ±30 %; up to 50 async and product-event invocations of an installation run at once; kills at the stated timeouts;
  resources served as committed; bundling like `forge deploy`; Forge SQL and Object Store are not available; the
  scoring sites use other seeds and the scale of §3." (1.2 KB)

`LIMITS.md` carries the numbers the invariants depend on, with the documented value or the benchmark's own value
labelled as such: cost table (writes 1; `issue/{id}` 2; `search/jql` and `issue/bulkfetch` 1 + issues returned;
`search/approximate-count` 1; user reads 1 + 2 per user; `myself`/`mypermissions` 3; `permissions/check` 3 + 1 per 100
ids; other GETs 1 + top-level objects; bridge `requestJira` 0 but burst-limited); header grammar and the three 429
variants plus 503; burst refill (GET/POST 100, PUT/DELETE 50 per second) and capacity (1 s of refill, a benchmark
value); per-issue windows; KVS/CES limits and the live-measured error codes (409 `KEY_CONFLICT`, 400
`CONDITIONAL_CHECK_FAILED`, 422 for > 25 operations, 400 `KEY_DUPLICATION_ERROR`, 400 `TOO_MANY_BATCH_ENTITIES`, 404
`SCHEMA_NOT_FOUND`, 400 `INCORRECT_PROPERTY_TYPE`, 429 `RATE_LIMIT_EXCEEDED`); query lag (≤ 2 virtual s, CTF from the
probe); cursors valid only in the invocation that produced them; async limits, retry schedule (1, 2, 4, 8, then every
15 min to 24 h), 100 KB per event for consumers with `timeoutSeconds` > 55 enforced at push; concurrency `limit` 1–50,
any number of keys; product-event retries ≤ 4; scheduled semantics; invocation timeouts and user-led limits; bridge
500/25 s; Realtime 50 ops/s; LLM 100 RPM, 500k TPM per model, the token estimator `ceil(bodyChars/4) +
max_completion_tokens`, credit rates (Haiku 10, Sonnet 30, Opus 50 credits per 1M tokens, $0.10/$0.50 per credit,
tier mapping an assumption for unpublished models, llm#32), error shapes; logs 100 lines per runtime minute and 200 KB
(excess dropped); time model latencies (Jira read 150 ms + 1 ms/object, write 200 ms; KVS 20 ms, query 30 ms, batch 40
ms, transaction 50 ms, + 5 ms per extra 10 KB unit; push 25 ms; Realtime 10 ms; cold start 400 ms, warm 5 ms).

---------------------------------------------------------------------------------------------------------------------

## F. Check registry, composition and bands

### F.1 Composition

`inner` = Σ tier weight × mean of the tier's available, non-diagnostic rows (plain means, 1.0 §8.1).
`earned = (0.92 · inner + 0.08 · gate · e_mean) × crit_mult`, where `gate` = fraction of K rows, V rows and
`l_lint_warnings` at exactly 1.0, and `crit_mult` = Π over unsuppressed fired criticals of `0.6 + 0.4 · severity`.
`final = min(earned, ceiling − 0.05 · (1 − earned))` (1.0's BAND_PULL, never sits on a cap).

| tier | weight | rows | per row |
|---|---:|---:|---:|
| L lint & deploy | .05 | 6 | .0083 |
| K platform currency | .05 | 7 | .0071 |
| P pipeline & concurrency | .15 | 14 | .0107 |
| R resilience & continuation | .12 | 12 | .0100 |
| M migration | .10 | 9 | .0111 |
| D Tier 1 dosing | .12 | 11 | .0109 |
| S security | .14 | 16 | .0088 |
| A Forge LLM | .08 | 10 | .0080 |
| F front Custom UI + boot | .09 | 11 (+1 diagnostic) | .0082 |
| U UI Kit admin | .06 | 8 | .0075 |
| V visual | .04 | 5 | .0080 |
| E economy slice (0.08 of earned) | — | 4 | mean |

Systems tiers (P, R, M, D) carry 0.49 of inner; security 0.14; surfaces and AI 0.27; lint and currency 0.10.

### F.2 Admission bands (ceilings; `passed` = exactly 1.0, available, not vacuous)

| band | ceiling | requires |
|---|---:|---|
| B1 deployable | 0.499 | `l_deployable`, `l_bundles_load` |
| B2 working core | 0.649 | `p_read_contract`, `p_quiet_exact`, `f_front_loads`, `u_admin_boot`, `m_app_data_preserved` |
| B3 production roster (graded) | 0.949 − 0.02·(n − 1), never below 0.649 | n = DEFECTS among the roster: every non-diagnostic row of P (except `p_read_contract`), R, M, D, S, A, plus `f_boot_*`, `f_bootstrap_scales`, `f_live_update`, `f_paused_state`, `u_team_form`, `u_table_sort` (≈ 80 rows); a row failed because a ROOT_BLOCKS root in the band failed is that root's shadow |

Monotone: the B3 floor equals the B2 cap, so passing B2 never scores below failing it. The prompt states the bands in
words (parity test, 1.0 §14).

### F.3 Criticals (floor 0.6 each; every one fires only on the defect it names, observed)

| critical | consequence named | fires only when | severity input |
|---|---|---|---|
| `l_deployable` | deploy blocked | any lint error (client + server-rule pack + pinned runtime flag) | 0 |
| `l_bundles_load` | app does not run | < 0.4 of functions bundle and load | 0 else 1 |
| `p_final_exact` | silent data corruption | C4: graded people not exact | `min(s, 0.5)` when s < 1 |
| `p_side_effects_once` | duplicate write on the customer's Jira | ≥ 2 comments for one move, or a move applied twice over a human's later change | 0 |
| `m_app_data_preserved` | customer configuration lost in the upgrade | a v1 team, capacity, policy, admin, absence or exclusion missing or wrong after C1 | `min(s, 0.5)` when s < 1 |
| `d_pool_not_exhausted` | cross-tenant outage | the ledger shows the pool hit 0 in an hour where other installations stayed within their stated draw, i.e. the app's own spend exhausted it | 0 |
| `s_admin_authz` | privilege escalation | a non-admin call changed admin state (semantic diff) | 0 |
| `s_cross_user_leak` | data leak | a hidden-issue canary reaches a person's response, DOM, prompt or Realtime payload | 0 |
| `s_cross_tenant_leak` | tenant isolation breach | an installation's canary appears in another installation's response, log, storage or prompt | 0 |
| `s_webhook_auth` | forged data accepted | an unsigned, mis-signed, tampered or stale delivery changed an absence, capacity or number | 0 |
| `f_front_loads` | dead primary flow | the board shows no person with numbers (diagnostic weight 0) | 0 else 1 |

Vacuity: every "no defect" critical needs its exercise (≥ 1 comment POST attempted, ≥ 1 admin resolver present, ≥ 1
person-facing list, ≥ 1 accepted signed delivery …) or it carries `vacuous_root` (0, no multiplier). A section that
cannot run because of the app's own manifest is `vacuous_root`, never a charged cliff (§17.8 G and comment-rows D4).

### F.4 The registry

"Anchor" = contract section and key words. "Measurement" names the load-robust instrument: **GL** gateway ledger
(virtual time, one row per product call), **SL** storage op log, **IL** invocation log (start/park/kill/crash/return,
origin tree), **QL** queue log, **RL** Realtime log, **LL** LLM log, **WL** web-trigger ingress log, **SD** semantic
state diff, **RC** read contract (`get-workload` as admin) after quiescence, **DOM** browser DOM after the page has
settled (no pending bridge op or fetch; event-based, no timeouts as grades), **FD** UI Kit ForgeDoc snapshot, **CDP**
boot waves/bytes/requests. All correctness rows are per world, worst world kept.

**L — lint & deploy (.05)**

| id | C | measured / pass rule | anchor |
|---|---|---|---|
| `l_deployable` | C | 0 errors: client lint (16 linters) + measured server-rule pack + pinned deprecated-runtime flag; lint ×2 identical | prompt done 1; STARTER lint |
| `l_bundles_load` | C | share of manifest functions that bundle and load under the wrapper | prompt done 1 |
| `l_lint_warnings` | | 1 − 0.2 per distinct warning | prompt |
| `l_real_packages` | | `@forge/*` resolve at kit pins; no imports outside the kit | STARTER packages |
| `l_scopes` | | declared = required per call (OpenAPI OAuth2 alternative rule of 1.0 §5.3), −0.25 per miss/extra | §2 scopes sentence |
| `l_deploy_readiness` | | fraction: one `jira:globalPage`, one `llm`, ≤ 1 `rovo:mcp`, ≤ 5 scheduled with ≤ 1 `fiveMinute`, RoA eligible (static triggers, no egress/remotes), no `unsafe-*` script CSP, no `sql`/`objectStore` | §2 RoA, §13 |

**K — platform currency (.05)**

| id | measured | anchor |
|---|---|---|
| `k_modules` | every §2 module present in a schema-valid current form (`dashboards:widget` with `edit`, consumer `function` form, `jira:adminPage` `render: native` with a file resource) | §2 table |
| `k_entities_indexed` | the reads that serve the board, action and recompute are entity queries on declared indexes (SL) | §2 storage |
| `k_uikit_admin` | `@forge/react` ≥ 10 in the admin bundle, no `@forge/ui`, no `react-dom`, classic-pragma-safe `.jsx` (uikit#23) | §10 |
| `k_current_apis` | no `@forge/api` `storage`, no `/rest/api/3/search`, every product call through `route`, nodejs22/24 | §2 |
| `k_llm_model_current` | every chat/stream names an id `list()` reports `active` at that virtual moment (one model turns `deprecated` mid-run) | §3 model list, §11 |
| `k_rovo_wiring` | skill frontmatter rules, agent lists the skill, mcp exposes `get-workload`, `actionVerb: GET` | §12 |
| `k_static_webtrigger` | `hr-feed` static with the four outputs; returns `{outputKey}` | §6 web trigger |

**P — pipeline correctness and concurrency (.15)**

| id | C | measured / pass rule | anchor | measurement |
|---|---|---|---|---|
| `p_read_contract` | | `get-workload` schema-valid for valid, unknown and missing inputs (root of every RC row) | §2, §12 | RC |
| `p_quiet_exact` | | I3 quiet window after C1: `loadHours`, `wip` exact for all people | §4 exact | RC |
| `p_burst_throughput` | | **plain** population on I1 exact at the bar (C2), fraction | §4 throughput | RC at virtual 09:38 |
| `p_hot_aggregate` | | **hot** population exact at C2 (lost-update detector) | §4 exact, §3 concurrency | RC |
| `p_concurrent_same_issue` | | **same-issue** population exact at C2 | §3 "concurrently", §4 | RC |
| `p_duplicates_idempotent` | | **duplicate-only** population exact at C2 | §3 "at least once" | RC |
| `p_out_of_order` | | **reorder-only** population exact at C2 | §3 "in any order" | RC |
| `p_world_changes_converged` | | **world-change** population exact at C4 (moves keep the id, deletes, project delete without child events, deactivation) | §3, §4 convergence | RC |
| `p_final_exact` | C | every graded person on every installation exact at C4 | §4 exact | RC |
| `p_side_effects_once` | C | exactly one comment per applied move; no move re-applied over a later human change | §4 exactly once, §11 | site write log |
| `p_idle_rerun` | | C5 hour: 0 storage writes; points ≤ the stated idle allowance (continuous on overage) | §4 idle | SL, GL |
| `p_jql_function_fresh` | | `issue in overloadedWork()` as two users = overloaded set ∩ each user's visibility, within 5 min of a change; no error with > 1,000 matching issues | §2 JQL | site search log |
| `p_realtime_coalesced` | | ≤ 50 Realtime ops per virtual second per installation during the burst; the open board converges after U2 without reload | §8 live, LIMITS | RL, DOM |
| `p_no_unawaited` | | 0 I/O requests issued after handler return (held-request count) | §4 nothing after return | IL |

**R — resilience and continuation (.12)**

| id | measured / pass rule | anchor | measurement |
|---|---|---|---|
| `r_kill_safe` | invocations killed at deadline leave no lost or doubled item (outcome over the affected populations) | §3 "end at any point", §13 timeouts | IL + RC |
| `r_crash_redelivery` | after seeded crashes mid-invocation (incl. after side effects) every item lands exactly once | §3 platform fault | IL + RC + site writes |
| `r_no_inflight_sleep` | no in-invocation wait > 5 virtual s (virtual timer log) | §5 | IL |
| `r_retry_after_honoured` | no repeat of a refused request before its Retry-After (or reset), ≤ 4 attempts, no blind non-idempotent retry (1.0's repeat definition, §17.6) | §5 retries | GL |
| `r_long_wait_deferred` | the ~1,140 s quota Retry-After: affected work completes after the reset with 0 product calls before it, across the 900 s `retryAfter` clamp | §5 quota | GL + QL |
| `r_poison_dead_letter` | one issue whose GET answers 500 forever per installation: it appears in `dead-letters` with a reason, ≤ 6 attempts per hour, all other work completes | §3, §10 Operations | GL + FD |
| `r_scheduled_dup_overlap` | duplicate and overlapping reconcile runs cause no doubled work or writes; a skipped tick leaves no gap | §3 scheduled | SL + RC |
| `r_install_race` | I4: backfill complete and exact by 10:05 despite 20 min of 403s; nothing lost | §3 new site | GL + RC |
| `r_push_limits` | no work item lost to push errors (50/push, 500/min, 200 KB, 100 KB long-consumer events, cyclic 1,000); fraction of items whose push was rejected and never processed | LIMITS | QL |
| `r_kvs_limits` | seeded `batchSet` partial failures and KVS 429s: every intended write lands | LIMITS | SL + SD |
| `r_cursor_free` | scans resumed after kills miss no item when cursors from an earlier invocation are refused | LIMITS cursors | SL + RC |
| `r_world_404` | a 404 for a deleted issue ends its processing (no redelivery chain > 2 attempts) | §3 deletes | QL |

**M — migration (.10)**

| id | C | measured / pass rule | anchor |
|---|---|---|---|
| `m_app_data_preserved` | C | teams, capacities, policies, admins, absences + tombstones, exclusions exact after C1 (**capacity**, **override** populations, admin panel FD, `availableHours`) | §7 |
| `m_secret_moved` | | no plaintext secret in any KVS value after C1 (scan for the canary); deliveries signed with it accepted | §7, §6 secrets |
| `m_v1_queue_drained` | | every queued v1 `{issueId}` event's change reflected by C1 | §7 queued v1 events |
| `m_during_migration` | | every sampled read between upgrade and flip is exact OR `state: migrating` with progress; a wrong number = 0 | §7 |
| `m_converged_by_deadline` | | I1–I3 exact (all fields) at C1 | §7 60 minutes |
| `m_no_stale_overwrite` | | **stale-overwrite** population: issues changed live during migration keep the newer state | §7, §4 |
| `m_migration_idempotent` | | duplicated `upgraded` delivery, migrator killed at its timeout: no doubled data, completes | §3, §7 |
| `m_v1_cleanup` | | no `load:*`, `cfg`, `ovr:*`, `abs:*`, `hook-secret` keys remain after C1 + 30 min | §7 |
| `m_catchup_economy` | | points spent by migration + catch-up per installation ≤ stated reference (CTF ×1.5), continuous | §7, §5 |

**D — Tier 1 dosing (.12)**

| id | C | measured / pass rule | anchor |
|---|---|---|---|
| `d_background_budget` | | per installation per virtual hour, background points ≤ 8,000 (CTF): `min(1, budget / worst hour)` | §5 budget |
| `d_pool_not_exhausted` | C | see F.3 | §5, §3 other installations |
| `d_pause_on_quota` | | after the wall's first 429 per installation: backend product calls before the reset (count → `1/(1+n)`) | §5 quota |
| `d_near_limit_reserve` | | after the first response carrying `r`: background product calls before the reset (count); user-initiated calls not suppressed | §5 reserve |
| `d_interactive_priority` | | I3 user actions during I1's burst/migration and in the reserve window succeed (fraction) | §5 |
| `d_burst_reaction` | | after a `jira-burst-based` 429 on endpoint E: E not repeated before Retry-After; other endpoints continue | §5 |
| `d_per_issue_writes` | | ≤ 20 writes/2 s and ≤ 100/30 s per issue; after `jira-per-issue-on-write` only that issue waits | §5 |
| `d_concurrency_cap` | | max backend product requests in flight per installation ≤ 10: `min(1, 10 / max)` | §5 |
| `d_schedule_spread` | | no 5-min window holds more scheduled-origin points than budget ÷ 4 (a cheap hourly run passes; a heavy job at :00 does not) | §5 spread |
| `d_self_accounting` | | Operations `ops-points-bg`/`ops-points-ui` within ±2 % (or ±10 points) of GL at C2, C3, C4 | §5, §10 |
| `d_header_variants` | | the app's `ops-paused-until` equals the reset after each 429 variant (Retry-After, gateway-only `X-RateLimit-Reset` with and without seconds, unknown reason) and after `r` in any policy order | LIMITS headers |

**S — security (.14)**

| id | C | measured / pass rule | anchor |
|---|---|---|---|
| `s_admin_authz` | C | every resolver key of every module (introspected from `@forge/resolver` definitions) invoked as member, lead, unlicensed and anonymous `unidentified` with fuzzed payloads: admin state SD = 0 and refusal returned | §6 roles, "callable directly" |
| `s_role_rules` | | lead cannot grant admin; self-escalation refused; last admin and self-revoke refused; Jira ADMINISTER holders are admins | §6 |
| `s_verdict_cache_ttl` | | Jira admin revoked / issue permission removed at t: refused from t + 5 min | §6 cache ≤ 5 min |
| `s_identity_from_context` | | payload `accountId`/`isAdmin`/`role` ignored; effects attributed to context | §6 |
| `s_issue_level_permission` | | drill-down, exclude, AI and apply honour issue-level security and project roles; check-id = act-id (IDOR: ids outside the proposal refused) | §6 permission rule |
| `s_cross_user_leak` | C | F.3 | §6 |
| `s_cross_tenant_leak` | C | F.3 (warm reuse, colliding keys) | §6 warm reuse |
| `s_webhook_auth` | C | unsigned, bad signature, tampered body, stale timestamp → 401 and SD = 0; case-varied valid header accepted | §6 scheme |
| `s_webhook_replay` | | the same delivery sequentially and concurrently: exactly one effect, `duplicate` output | §6 |
| `s_webhook_ordering` | | **absence** population: `availableHours` exact (LWW by seq, tombstones never resurrected) | §6 |
| `s_secret_rotation` | | old secret accepted inside the 10-min grace and refused after; new accepted from rotation | §6 rotation |
| `s_secret_hygiene` | | secrets in no response, log, prompt, Realtime payload or plain KVS value; admin read-back is presence only | §6 secrets |
| `s_rendering_safe` | | XSS canaries in summaries, team names and LLM output: no script execution, no `javascript:`/external links | §6 rendering |
| `s_path_safety` | | ids like `../../rest/api/3/…` in payloads hit no unintended endpoint | §6 |
| `s_realtime_isolation` | | a member subscribing to a lead's `ai:` channel receives nothing; client tokens cannot publish | §6 Realtime |
| `s_rovo_authz` | | action ignores identity inputs, refuses unlicensed, returns only what the caller may see | §12 |

**A — Forge LLM (.08)**

| id | measured / pass rule | anchor |
|---|---|---|
| `a_params` | no `temperature`+`top_p`, none on the models that reject either (documented knowledge trap, llm#16,17); `max_completion_tokens` on every call; forcing `tool_choice` | §11 (docs) |
| `a_runs_async` | proposals under 40–140 s latency are delivered (no resolver-bound LLM call); 0 LLM calls during any page boot | §11 |
| `a_validation` | malformed arguments → error state, no write; hidden, unknown, out-of-team and overloading moves dropped with a reason; digits-bearing summary replaced | §11 |
| `a_failures` | refusal, 403 forbidden model, 429, 500, a stream ending without a non-null `finish_reason`, an empty `end_turn` → stated UI state; partial never shown as final; no retry storm | §11, LIMITS shapes |
| `a_confirmation` | an injection-obeying answer (write tool call, hidden key, exfiltration URL) produces 0 Jira writes without the lead's confirm and 0 egress | §11 untrusted |
| `a_prompt_scope` | lead prompts contain no issue the lead cannot browse (non-critical; the leak critical covers person-facing output) | §11, §6 |
| `a_batch_dosing` | 144-job digest: ≤ 100 requests and ≤ 500k estimated tokens per model in every 60 s window, 0 requests in the 60 s after a 429, batch complete within 30 min (continuous parts) | §11, LIMITS |
| `a_cache` | identical rerun → 0 new LLM calls; one changed person → 1 call | §11 cache |
| `a_budget` | budget exhausted via the admin panel → 0 further calls + stated message; credits shown within ±1 % of LL × rates | §11, §10 |
| `a_label` | AI text carries `[data-testid=ai-label]` | §11 |

**F — front Custom UI (.09)**

| id | measured / pass rule | anchor | measurement |
|---|---|---|---|
| `f_front_loads` (C, diag.) | board shows ≥ 1 person with numbers | §8 | DOM |
| `f_numbers_dom` | DOM numbers = truth at U1 for member and lead | §8, §1 | DOM |
| `f_boot_shell` | shell at wave 0 | §8 boot | CDP waves |
| `f_boot_waves_ops` | READY ≤ 1 backend wave and ≤ 2 backend-bound ops | §8 | CDP + bridge log |
| `f_boot_bytes_requests` | ≤ 220 KB gz app-origin bytes, ≤ 6 requests, initiator depth ≤ 3 (CTF) | §8 | CDP |
| `f_bootstrap_scales` | bootstrap resolver: 0 Jira calls, ≤ 3 rounds, ≤ 6 storage ops on I3 AND I1 | §8 | IL + SL |
| `f_boot_429` | under an invoke 429: shell stays, exactly one retry at/after `rateLimitReset` | §8 | bridge log |
| `f_drilldown` | visible issues + `hidden-count` correct for member vs lead | §8, §6 | DOM |
| `f_live_update` | U2: converged numbers without reload, 0 polling invokes while idle | §8 | DOM + bridge log |
| `f_ai_panel` | U3 flow: proposal via Realtime, select, apply, per-move outcomes `applied`/`stale`/`refused`/`failed` match the site | §8, §11 | DOM + site |
| `f_paused_state` | U4: board loads from storage, drill-down via bridge works, AI shows `paused` with the reset time | §8, §5 | DOM |
| `f_widget` | needs-config, edit via the edit API + host Save, view, second instance, live update | §9 | DOM |

**U — UI Kit admin (.06)**

| id | measured / pass rule | anchor |
|---|---|---|
| `u_admin_boot` | ≤ 1 invoke before the first ForgeDoc holding `teams-table`; non-empty first ForgeDoc; quiescent after each action | §10 |
| `u_tabs_structure` | stated tabs and testIds present | §10 |
| `u_team_form` | invalid slug → stated message, 0 invokes; valid → 1 invoke with members as account ids; double-activate → 1 | §10 |
| `u_table_sort` | utilization column sorts numerically on stated decimal cases (host sorts by cell key; display strings like `85.5%` mis-sort as decimals do, uikit#11 as corrected) | §10 |
| `u_ops_values` | Operations fields present and formatted | §10 |
| `u_dead_letter_actions` | retry and discard → 1 invoke each; row state updates | §10 |
| `u_secret_form` | secret never echoed; `hook-configured` and rotation status shown | §10 |
| `u_roles_form` | grant/revoke flows and the refusal message for last-admin | §10 |

**V — visual (.04)**: `v_theme_tokens` (`theme.enable`, `--ds-text*`, contrast ≥ 4.5:1), `v_dark_mode` (surface
token family), `v_csp_clean` (0 violations), `v_console_clean`, `v_layout` (no horizontal overflow at 1280 and 900 px,
no clipped number). Anchor §8/§9 "Custom UI" paragraph, as 1.0 §7.

**E — economy slice (continuous against STATED reference numbers, never hidden optima)**: `e_scenario_points` (total
points per installation ÷ stated reference, `min(1, ref/x)`), `e_kvs_units` (write units for migration + burst ÷
reference), `e_llm_tokens` (mean input tokens per proposal ≤ stated 12,000), `e_boot_bytes` (`min(1, golden×1.2/x)`
with the golden's number published). Each reference is a number in LIMITS.md after calibration (§17.8 F: no cliffs).

### F.5 ROOT_BLOCKS (mechanical implication only)

`l_bundles_load` → every runtime row; `p_read_contract` → every RC-measured row; `f_front_loads` → F rows that need the
board; `u_admin_boot` → U rows and FD-read D rows (`d_self_accounting`, `d_header_variants`); `m_app_data_preserved` →
`p_jql_function_fresh` (the overloaded set depends on capacities). Everything else is separated by the disjoint
populations of D.5, so no other attribution is needed.

### F.6 Severity selftest (refuses the freeze on any inversion)

(1) every weighted row earns; (2) one lint warning costs points, never caps; (3) a cross-user leak scores below a
missing AI panel; (4) a duplicate comment scores below a missing comment; (5) a cross-tenant leak below a missing
widget; (6) empty manifest ≤ 0.05; **Loadline 1 unchanged ≤ 0.10** (brownfield idle control); (7) a dead bundle
multiplies once and lands ≤ 0.30; (8) data-loss transforms exact; (9) B3 ceilings exact at n = 1…17; (10) dominance;
(11) the single-defect cost table pinned in tests; (12) a measured zero-comment app scores ABOVE a duplicating one
(§17.8 B made a test).

### F.7 One-defect mutants (one per row; representative subset with expected losses)

| mutant | defect | expected loss |
|---|---|---|
| `m_rmw_aggregate` | snapshot/aggregate updated by get → add → set | `p_hot_aggregate` |
| `m_lww_no_version` | contribution upsert without a version condition | `p_concurrent_same_issue`, `p_out_of_order` |
| `m_dedupe_event_id` | idempotency on `eventId` | `p_duplicates_idempotent` |
| `m_push_per_event` | trigger pushes one queue event per update, `RateLimitError` unhandled | `p_burst_throughput`, `r_push_limits` |
| `m_serial_with_reads` | ingestion through one limit-1 consumer that reads every issue from Jira (≈ 200 virtual ms each) | `p_burst_throughput` (a serial consumer using event-carried state and batched writes would pass: the bar demands throughput, not parallelism) |
| `m_full_rescan_heal` | hourly heal rescans every issue | `d_background_budget`, `e_scenario_points` |
| `m_no_delete_detection` | heal only catches up by `updated` | `p_world_changes_converged`, `p_final_exact` (crit 0.8) |
| `m_sleep_retry_after` | waits Retry-After inside the invocation | `r_no_inflight_sleep`, `r_long_wait_deferred` |
| `m_retryafter_clamped` | `InvocationError({retryAfter: RetryAfter})` with no re-check on wake | `r_long_wait_deferred`, `d_pause_on_quota` |
| `m_numeric_retry_after_only` | ignores the gateway 429 without Retry-After | `d_pause_on_quota`, `d_header_variants` |
| `m_no_reserve` | ignores `r` | `d_near_limit_reserve`, `d_interactive_priority` |
| `m_blind_comment_retry` | re-POSTs the comment after a 503 | `p_side_effects_once` (crit) |
| `m_apply_no_intent` | apply without an intent record (crash re-runs) | `r_crash_redelivery` |
| `m_single_counter` | Operations points in one KVS counter | `d_self_accounting` |
| `m_unbounded_parallel` | `Promise.all` over 200 bulkfetches | `d_concurrency_cap` |
| `m_rename_v1_queue` | v2 drops the `load-events` consumer | `m_v1_queue_drained` |
| `m_blob_overwrites_newer` | migration writes v1 rows unconditionally | `m_no_stale_overwrite` |
| `m_show_v1_numbers` | serves v1 totals during migration without `migrating` | `m_during_migration` |
| `m_rebuild_from_scratch` | full rebuild instead of catch-up | `m_converged_by_deadline`, `m_catchup_economy` |
| `m_secret_kept_plain` | secret left in plain KVS | `m_secret_moved`, `s_secret_hygiene` |
| `m_lost_tombstones` | absence deletes remove the record | `s_webhook_ordering`, `m_app_data_preserved` (crit) |
| `m_persisted_cursor` | stores query cursors in job state | `r_cursor_free` |
| `m_display_condition_authz` | admin check only in `displayConditions` | `s_admin_authz` (crit) |
| `m_payload_identity` | trusts payload `accountId` | `s_identity_from_context` |
| `m_permission_cache_forever` | admin verdict cached for the session | `s_verdict_cache_ttl` |
| `m_hmac_eq` | parses and re-serialises JSON before HMAC | `s_webhook_auth` (crit) |
| `m_nonce_get_set` | replay check by get-then-set | `s_webhook_replay` |
| `m_module_cache_by_key` | module-level cache keyed by issue key | `s_cross_tenant_leak` (crit) |
| `m_asapp_prompt` | lead prompt built from all asApp issues | `a_prompt_scope`, `s_cross_user_leak` (crit) |
| `m_llm_promise_all` | digest fires all 144 calls at once | `a_batch_dosing` |
| `m_llm_in_resolver` | proposal computed inside the resolver | `a_runs_async`, `f_ai_panel` |
| `m_jql_id_list` | JQL function returns `id in (…)` | `p_jql_function_fresh` |
| `m_publish_every_update` | Realtime publish per contribution | `p_realtime_coalesced` |
| `m_unawaited_push` | `queue.push` not awaited | `p_no_unawaited` (+ items lost on recycled envs) |
| `m_readtime_board` | board bootstrap queries every member | `f_bootstrap_scales` |
| `m_await_before_render` | `await invoke()` before render | `f_boot_shell` |
| `m_userpicker_object` | sends the UserPicker object | `u_team_form` |
| `m_display_sort_key` | `"85.5%"` as the cell key | `u_table_sort` |

Phase-stopping mutants list their ROOT_BLOCKS rows; `forge_controls.py` (1.0, 90 % generic) applies, rebuilds,
scores and checks exactly the declared losses. Every platform-semantics mutant (races, KVS codes) must also misbehave
on real Forge in the L2 golden gate (§I), or it measures the emulator, not Forge.

### F.8 1.0's §17.8 lessons, now mechanisms

| lesson | 2.0 mechanism |
|---|---|
| A host chrome covered app controls; swallowed clicks | flags drawn outside the app frame with `pointer-events:none`; every probe click failure is recorded as harness evidence (`click_intercepted` → row unavailable, verdict held) |
| B critical fired on zero comments | criticals fire only on the named defect (F.3); targets are fresh, viewer-visible rows; selftest (12) |
| C schema-valid form never invoked | the engine invokes every schema `oneOf` arm (consumer `resolver:` form included) or the kit lint refuses it with a stated message |
| D contract text ≠ emulator | conformance suite: contract sentences about platform behaviour are tested against the engine by kit tests |
| E kit lint ≠ CLI | measured server-rule pack + pinned runtime flag; drift job |
| F one-call cliff | every economy/dosing row continuous |
| G roots charged twice | ROOT_BLOCKS + disjoint populations + priced-once band counting |
| H partial credit measured the wrong outcome | each fraction row measures the outcome it names; design text = code, enforced by a selftest reading this registry |

---------------------------------------------------------------------------------------------------------------------

## G. Difficulty argument

### G.1 What the transcripts say beats Forge 1.0, and where each lands here

Every semantic trap was "handled from the text by both models, first time"; "Testing found no semantic bug in either
app"; "All six defect classes Haiku found by testing were platform wiring, each announced by an explicit dev-tool
error string" (frontier-behaviour §0, §2). So 2.0 does not add semantics. It turns the conditions they never met into
stated requirements:

| frontier evidence (frontier-behaviour.md) | 2.0 requirement that bites | rows |
|---|---|---|
| Haiku C18: "I'm accepting the eventual consistency tradeoff here since concurrent writes both read fresh state anyway"; its `sprint-issue` upsert "is read, compare, write" | concurrent triggers and consumers race on contributions, snapshots and accounting counters; the DES makes the race happen | `p_hot_aggregate`, `p_concurrent_same_issue`, `d_self_accounting` |
| Pareto avoided the race by "serializing the whole site through one consumer (`limit: 1`)"; 1.0's contract had the trigger hand each relevant update to a queue (1.0 contract §2) | 10,000 updates at 500 events/min is 20 minutes of pushing, the bar ends 18 minutes after the first; a serial consumer that reads Jira per issue needs ≈ 33 minutes | `p_burst_throughput`, `r_push_limits` |
| "both reconcile designs rescan every issue updated since the earliest active sprint started … every hour"; Haiku "had designed the fix and dropped it" (`lastRunAt` watermark → `Math.min(...startDates) - 2 * DAY`) | a full rescan of I1 is 24,000 points against an 8,000 budget; deletions must be found without rescans | `d_background_budget`, `p_world_changes_converged`, `m_catchup_economy` |
| "both sleep through Retry-After inside the invocation (Haiku: 'which works fine since consumer invocations can run up to 900 seconds', while never setting `timeoutSeconds`)"; Haiku "parses numeric seconds only" | a ~1,140 s Retry-After exceeds the 900 s `retryAfter` clamp and every timeout; the gateway variant has no Retry-After at all | `r_long_wait_deferred`, `d_pause_on_quota`, `d_header_variants`, `r_no_inflight_sleep` |
| Pareto: "Never retry a successful POST; the UI coalesces a double click before invoking this resolver"; Haiku: "rather than adding backend-side deduplication"; "The harness only injects a clean 429 on the comment POST" | comment POSTs commit and answer 503 with Retry-After — and Jira's own rate-limiting page says of such 503s "you can handle them with similar retry logic" (tiers §4.1); crashes after the assignee PUT | `p_side_effects_once` (critical), `r_crash_redelivery` |
| Haiku "caches the Sprint field and each board's estimation field forever"; "Pareto's consumer throws on a 404, so a deleted issue is redelivered for 24 hours"; "both store issue keys, which go stale after a move" | projects deleted (no child events), issues moved (key changes, id stays), deletions, deactivations, policy edits mid-run | `p_world_changes_converged`, `r_world_404`, `s_verdict_cache_ttl` |
| "Neither app versions its storage … both treat KVS as a fresh store" | Loadline 1 data must be migrated live, exactly, with v1 events in flight and live writes racing the migrator | all of M |
| "The contract works as a test plan. Each injected fault is announced … the paraphrase is enough" | the contract states classes, envelopes and invariants; never counts, timings or targets | all |
| "The budget never bound. They used 72 and 50 of 150 calls" | ~4× the surface; testing concurrency is expensive; see J.4 | — |
| Haiku "never exercised the restricted viewer, the LLM failure branches … and it scored 1.0 on all of them" | the dev site does not announce: races give wrong numbers silently; only ledgers show dosing defects | P, D |

### G.2 Sol and Opus specifically: the most plausible failures

Their 1.0 trees are not on this machine; evidence is board rows (all 63 non-excellence rows at 1.0), wall times (Sol
791 s, Opus 1,588 s) and the transcripts of Haiku and Pareto, 0.0108 and 0.0319 below them. My predictions, with my
confidence that the row FAILS for Sol:

1. **Accounting and snapshot counters under concurrency** (~70 %): strong models put conditional writes on the
   obvious entity (the per-issue row) and still write "just a counter" as get-add-set; the hot population and
   ±2 % accounting expose it.
2. **Push-per-event ingestion** (~65 %): the canonical Forge pattern (and 1.0's own golden) is trigger → `queue.push`
   per update; at 10,000 updates it misses the bar, and an unhandled `RateLimitError` loses work until a heal.
3. **Quota wall longer than `retryAfter`** (~60 %): `InvocationError({retryAfter: Retry-After})` is clamped to 900 s
   and wakes into the wall; scheduled runs during the wall rarely consult the pause.
4. **JQL function at scale** (~60 %): `id in (…)` is the natural fragment; it breaks the 1,000-value cap and goes
   stale on every issue change; precomputation refresh is a separate API few will wire.
5. **Migration races** (~60 %): migrator writes v1 rows unconditionally over newer live rows; numbers shown during
   migration without `migrating`.
6. **Heal economy and deletions** (~55 %): a correct-but-full rescan blows the budget; a cheap `updated`-only heal
   misses deletes. Count-based anti-entropy is the non-obvious middle.
7. **LLM batch dosing** (~50 %): `Promise.all` over 144 jobs; TPM per model ignored.
8. **Ambiguous write** (~45 %): a 503 with Retry-After on a comment POST is retried — the docs nudge exactly that.
   This one is critical (×0.6).
9. **Bootstrap that scales** (~40 %), **v1 queue continuity** (~35 %), **verdict-cache TTL / last admin** (~30 %),
   **UI Kit details** (UserPicker `.id`, decimal sort keys: ~30 % each).

Expected roster defects for Sol ≈ 9–12 → B3 ceiling 0.949 − 0.02·(n − 1) ≈ 0.73–0.79. Inner ≈ 0.80–0.88. A critical
(most likely the duplicate comment) multiplies by 0.6.

**Predicted GPT-6.1 Sol: 0.50–0.78; most likely 0.66–0.74** (about 0.47–0.50 if it fires the comment critical).
Opus 5.5: similar range (0.52–0.80). Reference app: 1.000 by construction (calibration, §J.3). Mid-tier models
(Qwen3.8 Flash class): 0.35–0.60 (missing surfaces, several criticals, B3 floor). Weak models: ≤ 0.30 (B1/B2). That
is the spread the mandate asks for, and the acceptance pilot (NOW.md phase 7) is "clearly challenged" when Sol's
verdict shows ≥ 5 failed roster rows that are all stated engineering requirements.

### G.3 Why this is engineering judgment, not trivia or volume

- Every heavy row is a stated invariant whose naive implementation fails for a reason derivable from stated platform
  facts at the stated scale. There is no hidden optimum: every economy target is a published number.
- Every hard requirement admits several correct designs (the alt app uses different ones, §J.2): inline-trigger
  processing vs. batched dirty-sweeps; materialised snapshots vs. read-time aggregation with a cheaper board;
  conditional transactions vs. partitioned concurrency keys; count-based anti-entropy vs. per-project id hashing.
- Platform trivia is confined to K and a few A/U rows (≈ 10 % of inner) and each item is documented (sampling
  parameters, UserPicker object, sort by key, static trigger outputs, one range attribute, nodejs20).
- Volume does not pay: no row counts features; a model that builds fewer surfaces correctly beats one that builds
  all surfaces racily, because the systems tiers carry 0.49 of inner and the roster cap counts defects.

---------------------------------------------------------------------------------------------------------------------

## H. Fairness argument (how a capable model can know every graded thing)

1. **Everything graded is written down.** Behaviour: CONTRACT §1–§12. Numbers: LIMITS.md (budgets, bars, latencies,
   costs, limits, error codes). Shapes: `report.schema.json`, `v1/SCHEMA.md`, the HMAC scheme in full. The §14-style
   check↔contract map is a scorer test: a row without an anchor or an anchor without a row fails the build.
2. **Platform behaviour is stated as platform behaviour**, with its documented source, not as a fault list:
   at-least-once, no order, concurrency, kills, crashes, retries, push limits, cursor validity, query lag, 429
   variants. Each class occurs at least once on the dev world (kit test). Timings and counts are hidden; classes are
   not.
3. **Where Atlassian publishes no number, the benchmark publishes its own** and labels it: points per endpoint,
   burst capacity, the hard wall, other installations' draw, P_max = 50, query lag, latencies, retry schedule, LLM
   error shapes, credit rates for unpublished models. Rows grade compliance with those stated values, never knowledge
   of production's.
4. **Doc conflicts are pinned** (BRIEF §9.1): v2+ async retries; scheduled return value ignored (204); Retry-After as
   the only retry rule we grade; `r` only past 80 %; `rateLimitLimit` and `rateLimitValue` both returned; resolver
   25 s; SDK helper names; KVS query max 100; depth 31; nodejs22/24.
5. **The instruments are shared.** The dev kit's `world report` prints the same ledgers the grader reads. A model
   can see its points per hour, in-flight maximum, push rejections, Realtime rate and LLM windows before it submits.
   It cannot see the truth — but it can compute it from `forge-dev jira`, exactly as Pareto wrote its own tests.
6. **Exactness, not taste.** Numbers are integers-in-seconds rounded once; ordering, tie-breaks, null rules and
   rounding are stated; UI rows grade DOM hooks and ForgeDoc semantics, never pixels.
7. **Scale is stated.** The scoring envelope (4 installations, ≤ 30,000 issues, bursts ≤ 10,000 in 3 min, wall
   until reset, 403s ≤ 20 min) is in §3. The dev world is smaller but exhibits every class.
8. **No planted traps.** Loadline 1 is honest MVP code; its defects matter only through the data it leaves, whose
   format and guarantee are stated.
9. **Graded outcomes, never code shape.** No row requires transactions, a concurrency key, a particular queue or
   endpoint. The alt app proves it.
10. **Harness faults are never app evidence** (F.8): held verdicts, re-score, unavailable rows.

---------------------------------------------------------------------------------------------------------------------

## I. Fidelity plan

Architecture per real-forge-fidelity §4: L0 lint (client half byte-identical + measured server-rule pack), L1
**differential conformance** (the same probe source runs live on wolfaenpak and in the engine; any difference fails
the kit build; live answers committed as dated fixtures), L2 real deploy only as the golden gate at freeze, L3 browser
calibration in quiet windows. Nothing on real Forge is a score input. wolfaenpak is a sanctioned test site.

| # | emulated behaviour | live probe (same code live and in the engine) | settles | needed by rows |
|---|---|---|---|---|
| P1 | KVS/CES codes, atomicity, limits | existing `kvs-probe` (24 cases; today 13/24 match) | align the 11 differences (§3.1 receipts) | all storage rows |
| P2 | CES condition on a missing key; FAIL_IF_EXISTS on an entity `set` | extend P1 | create-if-absent idioms (machinery N5) | P, M, S webhook |
| P3 | query lag, `limit` 100/101, cursor reuse across invocations | `kvs-query-probe` (write → query loop; cursor in a second invocation) | stated lag value; cursor rule | `r_cursor_free`, recompute |
| P4 | optimistic conflicts under real concurrency | 50 events transact on one key; count `CONDITIONAL_CHECK_FAILED` | conflict codes under contention | `p_hot_aggregate` |
| P5 | KVS rate-limit error shape | short write burst on our own installation | `RATE_LIMIT_EXCEEDED` vs `TOO_MANY_REQUESTS` | `r_kvs_limits` |
| P6 | async retry schedule, `retryReason` values, `retryAfter` clamp, 4 KB `retryData` | `queue-probe` (~40 min idle wall) | the suspect `FUNCTION_ERROR`; first delays | R rows |
| P7 | delivery concurrency, concurrency key exactness, duplicates, order | 50 events with/without key, overlap measured | key semantics | P, R |
| P8 | push limits, 100 KB long-consumer events, cyclic 1,000 | 51 events; 201 KB; 101 KB event; chain to 1,001 pushes | enforcement point; error classes; N10 receipt | `r_push_limits` |
| P9 | timeouts, kill semantics, lowest-wins `timeoutSeconds`, remaining-time API | `timeout-probe` | partial writes persist; trigger timeout (25 s?) | R |
| P10 | product-event payloads: updated (assignee, `timeestimate`, status), created, deleted issue, deleted project, move, `selfGenerated` | capture real events on wolfaenpak | **whether event-carried state is complete** (decides the budget calibration), move/key semantics, GET of an old key | P, budget CTF |
| P11 | lifecycle installed/upgraded; upgraded only on major | install, minor deploy, major deploy of the probe app | payloads; non-delivery on minor [B] | M |
| P12 | scheduled return handling, per-installation offsets | passive, over hours | 204 handling; consistency of offsets | R |
| P13 | static web trigger: request shape (raw body string, header arrays, case), `outputKey` mapping, status codes | `webtrigger-probe` static variant | ingress fidelity (machinery N7) | S webhook, K |
| P14 | `jira:jqlFunction` input/output, precomputation storage and update via the computation API, 1,000-value behaviour | `jql-probe` | the whole JQL row | `p_jql_function_fresh` |
| P15 | Jira REST shapes used (search/jql paging, bulkfetch caps, approximate-count, user/bulk, permissions/check with security levels, comments ADF, bulk-edit task) | `rest-probe` reads (normal traffic) | mock shapes | P, S |
| P16 | rate-limit headers on normal traffic | passive capture of `RateLimit`/`RateLimit-Policy`/Beta- headers on probe calls | header grammar the app will see | D |
| P17 | Realtime pairing, claims, publish-only/subscribe-only tokens, error at > 50 ops/s | `realtime-probe` (browser leg, quiet window) | §17.8 D class | `s_realtime_isolation`, `p_realtime_coalesced` |
| P18 | Forge LLM validation errors, `list()` statuses, streamed tool calls, refusal, usage, 429 at 101 RPM | `llm-probe` (cents of tokens) | error shapes we state | A |
| P19 | UI Kit host callbacks and ForgeDoc | `uikit-probe` admin page; capture frozen as host fixtures | Form/Tabs/UserPicker arguments (uikit could-not-verify 2–4) | U |
| P20 | Custom UI served form, CSP, bridge headers since FRGE-1923 | `cui-probe` page | host fidelity | F, D |
| P21 | server lint rules for every 2.0 module and storage feature | lint corpus extended (~40 manifests) | rule pack | L |
| P22 | unawaited work after return | probe: an unawaited `kvs.set` after return, observed on later invocations | best-effort receipt; the documented semantics are what we emulate | `p_no_unawaited` |

**Not measured, and how the design avoids grading a guess:**
- Tier 1 wall, other installations' draw, burst capacity, per-endpoint costs, forgiveness: **stated benchmark model**
  (Atlassian's PM recommends exactly such a simulated test-quota layer, tiers#48; the docs forbid rate-limit testing on
  cloud tenants). Rows grade compliance with the stated model; header grammar is checked passively (P16).
- Exact retry intervals, P_max, query lag magnitude, scheduled duplicates/overlaps/skips, the install 403 window,
  latencies, LLM error codes, cold start: **stated**; rows grade outcomes with margins (bars ≥ 2× the golden), never a
  number the app had to guess.
- Log-cap overflow: stated as "dropped"; only "≤ caps" is graded.
- Pixels of either UI host: never graded.
- Every platform-semantics mutant must misbehave on real Forge in the L2 golden gate (deploy the golden and the race
  mutants to wolfaenpak with a scripted stimulus), else it is dropped.

---------------------------------------------------------------------------------------------------------------------

## J. Build plan, effort, risks, sizes, budget, cost, scoring time, packaging

### J.1 Work packages (2.0 lives in its own tree: `forge2/{public,starter,kit,world}`, `bench/score_forge2.py`,
`forge2_probe.mjs`, `forge2_oracle.py`, FORGE20 tier — integration.md D1)

| WP | owns | effort (agent-days) | reuse from 1.0 |
|---|---|---:|---|
| WP1 world engine | DES scheduler, worker environments + agent, virtual time, kills, crashes, unawaited freeze, KVS/CES (aligned), queues/jobs/concurrency, scheduled, product events, web-trigger ingress, lifecycle/upgrade | 12–16 | kvs.cjs ~95 %, emulator ~40 %, runtime wrapper pin, sandbox fence |
| WP2 mock services | mutable world timeline, indexed Jira (search/JQL subset + jqlFunction precomputations, bulk ops, permissions incl. security levels, users, project delete/move, ADF comments, commit-then-fail writes), gateway (ledger, pool + other installations, burst buckets, per-issue, headers), LLM script (streams, parallel tools, RPM/TPM), Realtime broker | 9–12 | site skeleton ~70 %, jql.cjs ~95 %, llm/realtime ~70 % |
| WP3 UI hosts | Custom UI host fixes (§17.8 A), wave scheduler + CDP counters; UI Kit host (bridge protocol, ForgeDoc → accessible HTML, async serialised callbacks, host state, ADS sort, UserPicker directory) | 7–10 | bridge-host ~85 % |
| WP4 lint | server-rule pack, runtime flag, CLI-equal walk, drift job | 2–3 | lint.cjs 100 % |
| WP5 scorer | probe timeline, oracle (indexed, O(issues) per checkpoint), 114 rows, composition, bands, criticals, selftest, calibration, controls, evidence diet | 12–15 | composition/bands/selftest/controls ~60 % |
| WP6 golden + v1 | Loadline 2 golden; Loadline 1 frozen app; v1 data generator | 8–10 | — |
| WP7 alt app | independent session, different designs (J.2) | 6–8 | — |
| WP8 mutants | ~100 one-defect patches + expectations | 4–6 | generator ~90 % |
| WP9 fidelity probes | P1–P22 live + conformance tests | 4–6 (+ ~3 h live wall, mostly idle) | kvs-probe |
| WP10 integration | FORGE20 tier, flags, per-era desktop/site tables (new letters P, M, D, F) | 4–6 | per integration.md |
| **total** | | **≈ 70–95** (≈ 4–5 weeks at four parallel lanes) | |

Order: WP9 P10/P13/P14/P19 first (they decide the budget calibration, the trigger, the JQL row and the UI Kit host);
WP1+WP2 core → WP6 golden green on the dev world → WP5 scores the golden → WP7 alt → WP8 mutants → freeze gate (golden
1.000 on both worlds ×3 seeds byte-identical; alt ≥ 0.95; mutants exact; controls; selftest) → phase 4 difficulty
proof → one GPT-6.1 Sol pilot.

### J.2 Reference and alt apps

- **Golden size:** ≈ 8,500 LOC in ≈ 70 files: backend ≈ 5,000 (gateway 500, storage 600, ingest 400, consumers
  1,000, scheduled 200, migration 400, security 300, webtrigger 250, LLM 500, JQL 200, Rovo 150, resolvers 500),
  board ≈ 1,600, widget ≈ 400, UI Kit admin ≈ 1,300, manifest ≈ 260 lines, SKILL.md ≈ 60.
- **Alt app (must score ≥ 0.95) uses different valid designs:** dirty markers keyed (issue, changelog id) + a
  `fiveMinute` sweeper pushing 50 issue ids per event; consumers keyed by issue-hash bucket (16 buckets, limit 1)
  instead of conditional upserts; read-time aggregation for `get-workload` and a per-team view rebuilt by a debounced
  job; points accounting as per-invocation ledger entries summed by a job; anti-entropy by per-project id hashing
  instead of approximate counts. If the alt cannot reach 0.95 inside the stated limits, a bar is too tight.

### J.3 Calibration (how the golden stays at 1.0 and the bars stay fair)

Every CTF number is set from the golden's measured value on 5 seeds with a stated margin: background budget = 1.6 ×
the golden's worst installation-hour (rounded up to 500); burst bar ≥ 2 × the golden's convergence; migration
deadline ≥ 2 × golden; boot bytes = 1.5 × golden; economy references = golden × 1.2. Calibration fails if the alt app
does not fit inside the same numbers. The pool arithmetic must stay consistent: other installations' stated draw is
set so that Σ background budgets + the interactive allowance + that draw ≤ 60,000 in every non-wall hour (a 5,000
margin under 65,000), which is what makes `d_pool_not_exhausted` attributable — an app within its budgets can never
exhaust the pool. If the live payload probe (P10) shows `timeestimate` is not in the event, the burst needs batched
reads (~10,000 points), the budget rises and the stated draw falls accordingly. Then the numbers are frozen into
LIMITS.md and `forge2-thresholds.json` (sha pinned, 1.0's `CALIB_SHA256` mechanism).

### J.4 Model-call budget (owner decides)

Estimated strong-model usage: read inputs 5–10 calls; explore typings/schema/OpenAPI/UI Kit 15–25; write ≈ 8,500 LOC
25–40; lint/wiring 10–20; dev-world testing of concurrency, migration, wall and UI 60–110; summary 1–2 → **≈ 120–210
calls** for a strong, test-driven run. 1.0's frontier used 50–72 of 150 calls for ≈ 2,000 LOC.
**Recommendation: 300 calls as a per-tier budget** (integration.md D5), not 150. Reasons: at 150 the budget would bind
the testing phase, and frontier-behaviour §5.3 shows a binding budget "would mostly punish exploration style … a
speed test, not a depth test"; 300 is ≈ 1.5–2.5× the expected frontier use, the ratio 1.0 had. If the owner keeps 150,
cut in advance (R3): drop the widget, the JQL function and the digest batch (−≈ 20 % of build calls), never the
pipeline, migration or dosing.

### J.5 Per-run model cost and wall time

Basis: 1.0 measured Sol at $0.72 / 13 min for ≈ 2,000 LOC, and Haiku's token profile (6.9 M prompt, 98 % cached, 207 k
output for 72 calls). 2.0 has ≈ 4× the code and heavier testing, and longer contexts.

| model | prices per M (in / out / cache read) | estimate per run | wall |
|---|---|---|---|
| GPT-6.1 Sol | $2 / $10 / $0.10 | **$5–12** (≈ 20–45 M prompt tokens ~95 % cached; 0.3–0.8 M output) | 50–90 min |
| Opus 5.5 | $4 / $20 / $0.20 | $10–24 | 90–150 min |
| cheap (GPT-6 Luna) | $0.10 / $0.50 / $0.01 | $0.40–1.00 | 60–120 min |

These are scaled estimates, not invoices; the Sol pilot replaces them with a measurement.

### J.6 Scoring time

World A: lint ×2 + server pack + build ≈ 1 min; backend timeline (≈ 400 k I/O steps through the DES at ~0.25 ms) ≈
2–4 min; UI phases U1–U6 with CDP counting ≈ 4–5 min; security battery ≈ 1 min → **8–11 min**. World B ≈ 5–7 min.
**Total ≈ 13–18 min per tree** (1.0: 6–7 min), serial on the scoring host, not load-sensitive in its grades (only in
its duration). A HELD verdict (watchdog) costs one re-score.

### J.7 Packaging

- Desktop payload (committed files): world engine ≈ 1.5 MB, scorer/probe/oracle ≈ 0.7 MB, Loadline 1 + generator ≈
  0.4 MB, public text ≈ 0.05 MB → **≈ +3 MB**.
- Kit cache (materialised once per kit version by `npm ci`, never shipped in the app, 1.0 §10): + the UI Kit closure
  (`@forge/react` 12.3.0 + reconciler + Atlaskit deps): **≈ +144 MB (registry sum), ≤ 306 MB on disk** — measure with a
  real `npm ci` + `du` before freezing (BRIEF §5.0). No SQL engine, no Confluence mock.

### J.8 Risks (ranked by my confidence that the design holds as written, lowest first)

| # | risk | confidence | mitigation |
|---|---|---|---|
| R1 | the DES with Atlassian's wrapper in worker threads (wrapper process assumptions; idle detection completeness) | MEDIUM-LOW | WP1's first task proves it on the 1.0 golden; child-process environments as the build-time alternative; byte-identical replay gate |
| R2 | UI Kit host fidelity (closed renderer) | MEDIUM | P19 capture as golden fixtures; allowlisted components with loud "unmodelled" |
| R3 | calibration leaves the golden no margin or lets naive designs pass | MEDIUM | 2× margins; alt app ≥ 0.95; every mutant must fail its row on both worlds; phase-4 difficulty proof |
| R4 | scorer complexity breeds 1.0-class scorer defects | MEDIUM | F.6 selftests, F.8 mechanisms, registry↔design selftest, independent red-team (phase 3) |
| R5 | public input ≈ 40 KB invites a desk audit | MEDIUM | tables moved to LIMITS.md; the prompt says the contract is a reference, not a checklist; open decision K2 |
| R6 | product-event payload lacks `timeestimate` (P10), making event-carried state impossible and the budget higher | MEDIUM | P10 first; the budget is CTF either way; the design holds with batched reads |
| R7 | scope vs 150 calls | MEDIUM | K1 (300), pre-decided cuts |
| R8 | scoring time 13–18 min and RAM for ≈ 50 workers per installation | MEDIUM-HIGH | workers share one process; P_max is a stated knob |
| R9 | fixture and oracle scale | HIGH | indexed generator and oracle by design (machinery N3 fixed) |

---------------------------------------------------------------------------------------------------------------------

## K. Open decisions for the owner

1. **Model-call budget:** 300 per tier (recommended) or keep the shared 150 with the pre-decided cuts (J.4).
2. **Public input ≈ 40 KB** (contract 23 KB + LIMITS 6 KB + others) vs. trimming to ≈ 30 KB by dropping the widget,
   the Rovo skill/mcp prose and the JQL function.
3. **Brownfield start** (workspace = Loadline 1, migration graded) — recommended; the alternative greenfield loses
   the whole M tier and the most novel axis.
4. **Benchmark-own rules** that are not production facts: the 8,000-points background budget, the near-limit
   reserve policy, the concurrency cap of 10, other installations' 30,000-point draw, the hard wall. Confirm they may
   be published as "this benchmark's rules".
5. **Eleven criticals** (1.0 had seven). Each fires only on its named, observed defect; compounding is real. Accept,
   or demote `d_pool_not_exhausted` and `s_webhook_auth` to roster rows.
6. **Two scoring worlds** (≈ 13–18 min) vs. three seeds (≈ 25 min).
7. **`jira:customField`** (GA, "constant performance regardless of the number of issues") as an extra systems row —
   only after a live probe measures when Jira invokes the value function.
8. **Confluence and Forge SQL excluded** (volume and fidelity, not difficulty) — confirm.
9. **Grade during-migration reads** as "exact or `migrating`, never another number" — recommended over requiring
   dual-read exactness (the golden must reach 1.0).
10. **RoA eligibility required** (static web trigger, no egress) — recommended; it is the platform's current
    direction and costs nothing in emulation once P13 lands.
