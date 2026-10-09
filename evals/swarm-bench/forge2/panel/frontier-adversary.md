# Forge 2.0 design proposal — "Signal Desk" (panel angle: FRONTIER ADVERSARY)

Independent panel design, 2026-10-09. Inputs read: `forge2/NOW.md`; `research/BRIEF.md` and the topic notes
(`tiers`, `robustness`, `security`, `llm`, `uikit`, `bootspeed`, `platform-2026-10-09`); `understand/frontier-behaviour.md`,
`real-forge-fidelity.md`, `machinery.md`, `integration.md`; Forge 1.0 `DESIGN.md` §2, §3, §8, §13.4–13.5, §17 and
`research/forge1-review/*.md`. Nothing was run except reads of the kit's pinned OpenAPI (`forge/kit/openapi/jira.json`) and
manifest schema (13.6.0), to confirm five facts this design leans on: `search/jql` has a `reconcileIssues` parameter
("Accepts max 50 ids") and says "Recent updates might not be immediately visible"; `POST /rest/api/3/issue` can set issue
properties at creation; `POST /rest/api/3/app/field/value` updates many issues' app-field values in one request;
`POST /rest/api/3/permissions/check` takes an `accountId` ("If no account ID is provided … the logged in user"); and the
13.6.0 shapes of `jira:customField` (`readOnly`, `type: number`), `jira:jqlFunction`, `jira:adminPage` (`render: native`),
`webtrigger` and `consumer` (three schema arms, including the deprecated `resolver` form).

Evidence markers used throughout:
- **[DOC]** an Atlassian-documented fact confirmed in BRIEF (topic tag given);
- **[MEAS]** measured on wolfaenpak;
- **[OAS]** stated in the shipped OpenAPI;
- **[BENCH]** a value the benchmark states because Atlassian publishes none;
- **[J]** graded by accepting every reasonable reading.

---------------------------------------------------------------------------------------------------------------------

## 0. The angle in one page

Forge 1.0 was beaten by transcription. From `frontier-behaviour.md`:
- "Every semantic trap that both models handled traces to a contract sentence that Haiku paraphrased in its own design
  notes. Testing found no semantic bug in either app."
- "The contract works as a test plan. Each injected fault is announced."
- "All six defect classes Haiku found by testing were platform wiring, each announced by an explicit dev-tool error string."

Sol and Opus 5.5 scored 1.0 on all 63 non-excellence rows. Their only loss was an economy target the contract never
stated. Adding more of the same (modules, trivia, tighter numbers) would not change that; the report says so explicitly
(§5.3).

**This design changes how requirements are written, not only what they are.** Three rules:

1. **The contract states GUARANTEES and PLATFORM FACTS, never the harness's test plan.**
   - A guarantee is an outcome the customer relies on. For example: "a fingerprint never has two open escalation
     issues, whatever happens to deliveries, invocations and Jira's answers."
   - A platform fact is a property of the world, written as a property of the world. For example: "Jira may commit a
     write and still lose its answer."
   - The contract never says when, where or how often the harness exercises a fact. The model must apply every fact to
     every code path it writes.
2. **Every hard requirement is a COUPLING of stated guarantees that a paraphrase cannot satisfy one at a time.** Each
   pair below pulls in opposite directions:
   - saving Tier 1 points pushes toward caching, while 60-second permission freshness pushes against it;
   - exactly-once pulls against concurrency, lost answers and search lag;
   - a throughput bar pulls against race-free serialization;
   - caching for economy pulls against warm-process tenancy;
   - migrating old data pulls against serving it without a gap.
   Resolving each pair is engineering judgment. Transcribing the contract does not resolve it.
3. **Faults are keyed to DOMAIN ENTITIES, not to call order.** For example, "the first create attempt for fingerprints in
   a seeded set loses its answer". So every implementation meets the same adversity, however it orders its calls. The
   dev site reproduces every class, and it does not announce semantic errors. Like production, it shows duplicate issues
   and wrong counts silently; only platform errors produce error strings.

### 0.1 Observed frontier behaviour → stated 2.0 guarantee

| what the transcripts show (quoted from frontier-behaviour.md) | what 2.0 states (never as a trap) | graded by |
|---|---|---|
| Haiku C18: "I'm accepting the eventual consistency tradeoff here since concurrent writes both read fresh state anyway". Pareto serialised the whole site with `limit: 1` | Fact: consumers run up to 16 at once per installation unless you set a concurrency key [BENCH], and KVS writes are last-write-wins [DOC rob#44]. Guarantees: exact occurrence counts; one escalation per fingerprint; alerts reflected within 3 virtual minutes during storms | `i_no_lost_alerts`, `i_one_escalation`, `i_exact_counts`, `i_reflect_3min` |
| Haiku planned "persist a `lastRunAt` watermark…" and shipped a full rescan every run; its no-op rerun "issued 458 entity gets for 95 rows" | Guarantees: a per-installation background cap of 12,000 points per virtual hour; stated scale bounds; continuous economy rows | `d_background_cap`, `e_points_per_alert`, `e_kvs_units_per_alert` |
| Haiku: "which works fine since consumer invocations can run up to 900 seconds" (it never set `timeoutSeconds`); `Number(res.headers.get('retry-after')) \|\| 1` | Facts: quota walls last until the top of the virtual hour, so Retry-After can be up to 3,600 s [DOC tiers#7, BENCH hard wall]; the gateway 429 has no Retry-After [DOC tiers#15]. Guarantees: deferred work completes after the wall; no function waits a limit out | `t_wall_defer`, `t_no_wait_in_function`, `t_retry_timing`, `d_quota_pause` |
| Pareto: "Never retry a successful POST; the UI coalesces a double click before invoking this resolver". Haiku: "rather than adding backend-side deduplication" | Facts: resolvers run concurrently (two tabs, two people, a double click); Jira may commit a write and lose the answer [BENCH]. Guarantee: one acknowledgment, one comment, one chat message per step | `u_ack`, `i_no_dup_writes`, `i_notify_once` |
| Haiku: `if (key in cfg.boardEst) return;` (config cached forever). "Pareto's consumer throws on a 404". "Both store issue keys, which go stale after a move" | Fact: the world changes while the app runs (moves, deletes, resolutions, lost access, a second admin's edits, the model list). Guarantees: per-change outcome rules and a 60-second access-freshness bound | `w_moved`, `w_deleted`, `w_resolved`, `w_access_change`, `w_rule_change`, `w_model_deprecated` |
| "Neither app versions its storage." | Fact: installs upgraded from 1.x hold 1.x data and 1.x queued events (layout shipped). Guarantee: the same issue continues, nothing is counted twice, and secrets are moved | `m_*` |
| Haiku relied on "a KVS-cached config written by the scheduled job that runs before any updates are delivered" | Nothing in the contract orders the harness's work. Events can arrive before any scheduled run, during the install refusal window, and during migration | `t_wall_defer`, `m_same_issue` |
| "Neither model explored the dev site's data or event shapes before writing code." "Haiku never exercised the restricted viewer, the LLM failure branches…" | The dev kit reproduces every class (`forge-dev scenario`) and announces no semantic error. The prompt says so | everything; recall is reported, not scored |

---------------------------------------------------------------------------------------------------------------------

## A. Product pitch — Signal Desk

**One sentence.** Signal Desk turns the alert firehose from any monitoring tool into exactly one Jira escalation per
problem. It counts occurrences, routes and prioritises them, runs the SLA clock and tells the on-call room once. It keeps
working when an outage makes the firehose a storm, which is exactly when everything else breaks.

**Who pays and why.** Every engineering organisation on Jira Cloud pipes alerts into Jira, and the naive integrations
fail in two ways:
- they open a ticket per alert, so a 20-minute outage buries the board under 6,000 issues;
- they hammer Jira until the app's shared Tier 1 quota is gone and every customer of the app is locked out for the rest
  of the hour. A partner reported exactly that: "rendering the app unusable for up to one hour on all tenants" (T99654).

Signal Desk's value lies in the properties that are hard to engineer:
- deduplication under storms, at-least-once delivery and lost answers;
- dosing a cross-customer quota it cannot see;
- serving each person only what they may browse;
- AI triage that never acts on its own;
- an upgrade path that keeps the history 1.x customers already have.

**Surfaces.**
- **Responders** use a fast live console (Jira global page, Custom UI): what is burning, sorted by SLA, with
  acknowledge and an "Ask Signal Desk" assistant.
- On every escalation issue, a **Signal panel** (Custom UI) shows the occurrence history, the AI triage suggestion
  (applied only on a click) and the notification steps.
- **Admins** run a UI Kit 2 panel in Jira settings: integrations and secret rotation, routing rules, responders,
  budgets (Tier 1 points, LLM credits), live consumption, failed work and migration progress.
- **Rovo** can read and acknowledge escalations.
- **JQL** gets `issue in signalStorming()`.
- A read-only **`Signal occurrences` field** makes counts searchable.

**Why it is not benchmaxxed.**
- There is no tutorial, sample or hello-world.
- The domain (alert deduplication, SLA, storm handling) is mainstream operations software. What is rare is getting it
  right on Forge's limits.
- The app is graded only by running it against a world that behaves like production.
- The difficulty is the platform's real behaviour: shared quotas, at-least-once delivery, eventual consistency, warm
  reuse and ambiguous writes. It is not invented puzzles.

---------------------------------------------------------------------------------------------------------------------

## B. Modules

Status is cited from BRIEF §1.1 (verified 2026-10-09). "Hard part" names the engineering judgment, not the API trivia.

| # | module / API | status | role in Signal Desk | why needed | what makes it hard |
|---|---|---|---|---|---|
| 1 | `webtrigger` (dynamic) | GA [DOC sec#15–18] | signed alert intake `signal-intake` | the only inbound path for external systems | no platform authentication, so the app implements HMAC, a timestamp window, rotation grace and delivery idempotency (concurrent re-sends); it must answer in ≤ 10 virtual s while doing no Jira work; backpressure. Platform `hmacSharedSecret` is neither required nor penalised (BRIEF §1.1) |
| 2 | `consumer` + Async Events (`@forge/events` 3.0.7) | GA [DOC rob#15–32] | `ingest`, `materialize`, `flush`, `triage`, `ask`, `notify`, `migrate` | work too large or slow for one invocation | at-least-once delivery in any order; kills at `timeoutSeconds` with partial writes; 500 events per minute per installation shared by every queue; 100 KB per event for long consumers; cyclic limit 1,000; concurrency keys scoped to the installation across queues; `InvocationError` retries; v1-shaped bodies |
| 3 | `scheduledTrigger` ×3 (`fiveMinute` sla, `hour` heal, `day` housekeeping) | GA [DOC rob#38–41] | SLA stages, reconciliation, retention | SLA and healing lost events | at most 5 triggers, at most 1 `fiveMinute`; duplicates, skips and overlaps; a throw is not retried; spread work across the hour; no Jira reads for SLA (the app knows its own deadlines) |
| 4 | `trigger` (`avi:jira:updated:issue`, `avi:jira:deleted:issue`, `avi:forge:installed:app`, `avi:forge:upgraded:app`) | GA [DOC rob#33–37] | escalation state from humans' changes; bootstrap; upgrade | resolution, move and delete happen in Jira | events are late, duplicated, out of order or lost; `ignoreSelf` or `selfGenerated` (feedback loops); the installed event comes before permissions; the upgrade event comes only for major upgrades |
| 5 | `jira:globalPage` (Custom UI) | GA, one per app [B] | the responder console | front-facing surface | count-based boot budget (one backend wave); live through Realtime with no polling; visibility-scoped list in constant backend calls; acknowledge exactly once |
| 6 | `jira:issuePanel` (Custom UI) | GA [B] | the Signal panel on escalation issues | per-issue history and AI triage | `context.extension` ids are hints, not proof (IDOR); issue-level EDIT for applying triage; one backend op to READY |
| 7 | `jira:adminPage` with `render: native` (UI Kit 2, `@forge/react` 12.3.0) | GA [DOC uikit#18–20] | admin panel (Tabs; Router allowed, not required) | the owner's mandate | UI Kit host semantics (`useForm`, `onSubmit` without data, UserPicker objects, DynamicTable sort keys); every admin resolver authorised server-side; optimistic concurrency between two admins; secret shown once |
| 8 | `llm` (`@forge/llm` 1.0.7) | GA 2026-07-29 [DOC llm#1–38] | background triage (forced tool) and interactive Ask (agentic loop) | the owner's mandate | no structured outputs, so validate forced-tool arguments; parallel tool calls; 100 RPM and 500k TPM per model; model list changes mid-run; truncated streams; no Retry-After; budgets; cache scope; prompt injection from attacker-controlled alert text |
| 9 | Realtime (`@forge/realtime` 1.0.1; publish-only and subscribe-only tokens CHANGE-3326) | GA [B] | live console; Ask answers to the asker only | no polling (Atlassian anti-pattern, rob#73) | 50 ops/s per installation; `publish` returns `errors[]` and does not throw; server-derived claims (the exploitable LLM+Realtime tutorial, llm "newer facts" 5); `publishGlobal`↔`subscribeGlobal` pairing (1.0 defect D) |
| 10 | KVS + Custom Entity Store (transactions, conditions, batch, `keyPolicy`) | GA [DOC rob#43–59, MEAS] | the escalation ledger | the only storage (no SQL, by design) | query is eventual and `get` is strict; 409 `KEY_CONFLICT` [MEAS]; ≤ 25 ops per transaction; conditions only on CES; no `keyPolicy` in batch or transact; 4,000 + 4,000 10 KB units per minute; cursors unstable; one range attribute per index [MEAS server rule] |
| 11 | `jira:customField` (`type: number`, `readOnly: true`) + `POST /rest/api/3/app/field/value` | GA [B, OAS] | `Signal occurrences`, searchable in JQL | counts must live in Jira | per-issue write limits (20 per 2 s, 100 per 30 s) [DOC tiers#13] against occurrence storms; bulk writes; freshness ≤ 3 virtual min |
| 12 | `jira:jqlFunction` `signalStorming` | GA [B] (1.0 dropped it; contract now captured) | `issue in signalStorming()` | storm search for humans and Rovo | precomputations are shared across users and not refreshed unless the app updates them; at most 1,000 right-hand values (tenant A has more than 1,000 storming escalations); several valid designs exist (§C.3) |
| 13 | `action` ×2 (`get-escalation` GET, `ack-escalation` UPDATE), `rovo:agent`, `rovo:skill`, `rovo:mcp` | action and agent GA; skill Preview 2026-10-02; mcp Preview [B] | Rovo reads and acknowledges | latest modules; requirement 13 (inputs untrusted) | identity from context, never inputs; issue-level permission; accurate `actionVerb`; skill frontmatter rules |
| 14 | egress `https://relay.signal-desk.example` + secret store | GA [DOC sec#19–23] | chat notifications (benchmark-defined "Relay" API, OpenAPI shipped) | the external side effect that must happen exactly once | `Idempotency-Key` stable per logical step, not per attempt; 429/503 with Retry-After; accepted-but-answer-lost; token in `kvs.setSecret`. Runs on Atlassian is not required (egress); see K-5 |

**Deliberately out:**
- **Forge SQL.** It needs a MySQL engine in the payload. An atomic `INSERT … ON DUPLICATE KEY UPDATE` would also dissolve
  the concurrency problem this design is built on.
- **Confluence.** A second Atlassian mock with storage-format and restriction-inheritance fidelity risk, and its data would
  not interact with Jira's (frontier report §5.3 "mostly volume").
- **`dashboards:widget`.** Covered by 1.0; it adds volume, not judgment.
- **Workflow rules** (Preview; expression evaluator needed), **`apiRoute`** (request shape undocumented),
  **app-managed permissions** (unmeasured host semantics, machinery N6), **Object Store** (pre-signed host),
  **`global:fullPage`** (Preview; `jira:globalPage` suffices).
- **Multi-entry Custom UI resources** are allowed but not required, until bridge injection into named entries is proven
  on wolfaenpak (BRIEF §1.1 risk).

---------------------------------------------------------------------------------------------------------------------

## C. Architecture

The contract prescribes outcomes and hooks, not this architecture. Below is the REFERENCE (golden) design, which proves
the guarantees are jointly satisfiable within the stated limits. §C.10 lists alternative designs that also pass.

### C.1 Data model (golden; 12 CES entities ≤ 20, every named index has exactly one range attribute)

| entity | key | attributes (type) | indexes (partition → range) | why |
|---|---|---|---|---|
| `fpstate` | `fpHash` (sha-256 hex of the fingerprint) | gen (int), state (`none\|reserving\|open\|closed`), issueId, token, tokensTried (any), integrationId, version (int) | — (read only by `get`, never query) | the single strongly-consistent pointer per fingerprint; every create and close is a CES conditional transaction on it |
| `escalation` | `fpHash#gen` | fingerprint, integrationId, projectId, issueId, issueKey (display only), severity, slaStage (int), slaDue (int), ackBy, ackAt (int), openedAt (int), closedAt, dirty (int), triageState, version (int) | `by-due` state → slaDue; `by-project` projectId → slaDue; `by-issue` issueId → openedAt | SLA sweep without Jira; visibility-scoped console in constant calls; product events located by issue id |
| `occurrence` | `fpHash#gen#alertId` | fpHash, gen, at (int), deliveryId | `by-esc` [fpHash, gen] → at | idempotent by construction: re-writing the same row is harmless, so batches need no `keyPolicy`; exact counts are a recount, not a counter |
| `delivery` | `deliveryId` | integrationId, state, chunksDone (int), alertCount (int), receivedAt (int) (TTL 2 h) | — | delivery-level exactly-once and resume-after-kill checkpoint |
| `points` | `hour#installClass#shard` | spent (int), version (int) | — | sharded, conditionally incremented self-accounting (caps under concurrency) |
| `personpts` | `hour#accountId` | spent (int), version (int) | — | per-person cap under a concurrent masher |
| `person` | `accountId` | role (`responder`), grantedBy, version | — | app role store |
| `rule` | `ruleId` | order (int), labels (string, canonical JSON), projectKey, severity, version (int) | `by-order` const → order | optimistic concurrency between admins |
| `notification` | `escKey#step` | state, idemKey, attempts (int), lastError | — | exactly-once chat steps |
| `airesult` | `contentHash#family` | severity, category, summary, createdAt | — | triage cache (content and model family) |
| `failed` | `workId` | kind, reason, ref, at (int) | `by-at` const → at | the visible failed-work list |
| `integration` | `integrationId` | name, projectKey, room, hasToken (bool), secretVersion (int), graceUntil (int), legacy (bool) | — | integration metadata; secrets live in the secret store |

KVS (untyped):
- `cfg:*`: budgets, the background pause switch, the last pool headers seen, the model cache with its fetch time;
- `migration:*`: watermark key range and counts;
- `v1:*`: the 1.x layout, read-only until migrated, then deleted;
- `kvs.setSecret`: `sec:int:<id>:<version>` and `sec:relay:<id>`.

### C.2 Async topology

```
external sender ─ signed POST ─▶ webtrigger `signal-intake`  (55 s limit; answers ≤ 10 virtual s, no Jira calls)
   verify HMAC over raw body (secret store, rotation grace) → timestamp window → claim deliveryId (FAIL_IF_EXISTS)
   → store delivery body (KVS) → push 1 `ingest` event {deliveryId}  → 202 / 200 duplicate / 401 / 404 / 503+Retry-After
                         │
                         ▼
consumer `ingest` (timeoutSeconds 300; no key: concurrent)
   route by the rules version in force at acceptance → occurrence rows (batchSet, 25 per batch)
   → fpstate create-if-absent (CES conditional tx) → escalation.dirty++ (conditional) → checkpoint chunksDone
   → if remaining time or the background budget is short: checkpoint, then RETURN InvocationError(retryAfter)
   ├─▶ `materialize` (≤ 10 new fingerprints per event; key "jira-create", limit 4)
   │      reserve token (tx on fpstate: none→reserving, token=T) → POST /issue with property signal-desk {fingerprint,
   │      integration, token: T} → adopt id (tx reserving→open). Lost answer → InvocationError(retryAfter ≥ search lag)
   │      → search `issue.property[signal-desk].token = T` (and every token tried) → adopt, or a new token and retry
   ├─▶ doorbell `flush` (≤ 1 pending: KVS flag with TTL; key "flush", limit 1; ≥ 60 s cadence while dirty)
   │      exact recount of dirty escalations (by-esc query after the 5 s query lag) → POST /app/field/value (≤ 200 combos)
   │      → storm comments at 100 / 1,000 (claim then post, lost-answer safe) → Realtime publishGlobal of opaque ids
   │      → JQL freshness (field-based fragment needs nothing) → clear dirty only if unchanged (conditional)
   ├─▶ `triage` (≤ 5 escalations per event; key "llm", limit 2; paced ≤ 90 RPM, token estimator, daily budget)
   └─▶ `notify` (key "relay:<integration>", limit 2) → Relay with Idempotency-Key = <escKey>:<step>
product events (ignoreSelf) ─▶ trigger `issue-events` → escalation state by issue id (no Jira calls; the payload has fields)
lifecycle ─▶ trigger `lifecycle` → schedule bootstrap / migration (no Jira calls in the handler)
scheduled `sla` (fiveMinute) → `by-due` query → claim stage → comment + notify (lost-answer safe)
scheduled `heal` (hour) → pushes 12 delayed slice events (5-minute spacing plus per-installation jitter) → incremental
   `search/jql` over escalation issues updated since the watermark, plus a bulkfetch of a rotating 1/12 of the open set
resolver `ask.start` ─▶ `ask` (timeoutSeconds 300; key "ask:<accountId>", limit 1) → LLM tool loop (as the asker)
   → publishGlobal on `ask:<jobId>` with the publish-only token the resolver minted (asker's claims)
upgrade ─▶ `migrate` (timeoutSeconds 900; key "migrate", limit 1; key-range watermark, never a stored cursor)
```

**Continuation across invocation limits.** Every consumer is written as a resumable step function:
- it reads `getAppContext().invocationRemainingTimeInMillis()`;
- it checkpoints before each external call batch;
- when it is out of time or budget, it RETURNS `InvocationError({retryAfter})` (≤ 900, re-enqueued fresh past that).

It never sleeps through a limit. Long waits (a quota wall until the top of the hour) re-enqueue with `delayInSeconds`
≤ 900 in a chain, counted against the cyclic limit of 1,000.

**Event budget.** 500 events per minute per installation is shared by every queue, so the golden budgets it. In the
storm hour it pushes about 80 `ingest`, about 31 `materialize` (batched), at most 1 `flush`, about 62 `triage` (batched)
and about 20 `notify` per minute, which is ≤ 200 per minute. One event per alert would be 4,000 per minute.

### C.3 Resolver catalogue

**Admin resolver** (function `admin-fn`, used by the UI Kit page). The **admin API is STATED in the contract** (keys,
payloads, results), for three reasons:
- the harness can configure a world and probe security without depending on the UI Kit render;
- the UI Kit lane then grades the panel driving the same API;
- it is the "REST surface covering every UI action" a real vendor ships.

Rules for every key:
- Owner only. An Owner is a Jira administrator, checked live: `POST /rest/api/3/permissions/check` with
  `globalPermissions:["ADMINISTER"]`, or `mypermissions`, asUser. A verdict may be reused for at most 60 virtual s.
- A refusal is `{ "error": "forbidden" }` with no other effect.

| key | input | result | notes |
|---|---|---|---|
| `admin.state` | `{}` | `{integrations[], rules[], responders[], budgets, usage, failed[], migration}` | secrets never included (`hasToken: bool`) |
| `admin.createIntegration` | `{name, projectKey, room?}` | `{id, url, secret}` | `url` = the `webTrigger.getUrl` base + `/i/<id>`; `secret` shown here once |
| `admin.rotateSecret` | `{id}` | `{secret}` | the old secret keeps verifying for 10 virtual min |
| `admin.setRelayToken` | `{id, token}` | `{ok: true}` | stored with `setSecret`; never returned |
| `admin.deleteIntegration` | `{id}` | `{ok: true}` | its URL answers 404 afterwards |
| `admin.saveRule` | `{id?, labels: string[], projectKey, severity, version?}` | `{id, version}` or `{error:"stale"}` | a `version` older than stored changes nothing |
| `admin.moveRule` / `admin.deleteRule` | `{id, order?, version}` | `{version}` or `{error:"stale"}` | |
| `admin.grantResponder` / `admin.revokeResponder` | `{accountId}` | `{ok: true}` | effective within 60 virtual s |
| `admin.setBudgets` | `{llmDailyCredits, askPerPersonPerDay, backgroundPaused}` | `{ok: true}` | |
| `admin.replayFailed` | `{id}` | `{ok: true}` or `{error}` | replays exactly once |
| `admin.usage` | `{}` | `{hour, points:{background, people}, pool:{remaining\|null, resetAt\|null, paused}, rejected:{quota, burst, perIssue, other}, llm:{credits, usd}, kvsUnits, events}` | compared with ground truth (`d_headers`, `u_admin_usage_failed`) |

**Console resolver** (function `desk-fn`; keys chosen by the app; graded through the UI and by replay).

| golden key | auth rule | backend work | failure handling |
|---|---|---|---|
| `desk.bootstrap` | `context.accountId` only. Visible projects come from one `permissions/check` (projectPermissions BROWSE, the asker's `accountId`, asApp); issue security is checked by one bulk `permissions/check` on the ≤ 50 × P candidate issue ids | KVS `by-project` queries (≤ 1 page per visible project, P ≤ 10); a realtime subscribe-only token with server-derived claims | ≤ 3 sequential rounds; quota-paused gives `{paused, retryAt}` with KVS data still shown; never throws |
| `desk.ack` | Responder role (≤ 60 s verdict) + BROWSE + ADD_COMMENTS on that issue for `context.accountId` (bulk check). Payload identity ignored | conditional tx on `escalation` (ackBy unset → set); comment asUser (ADF: "Acknowledged by" + a mention node of the person); `notify` push | idempotent per escalation. A concurrent or later ack returns `{status:"already", by}`. A lost comment answer leads to a lookup of the app's own comment marker (ADF `localId`) before any re-post |
| `desk.ask` | any viewer; per-person Ask budget | mint a publish-only token (claims: accountId, jobId); push `ask` with the token | `{jobId}`; refusals `{error:"budget"}` / `{error:"rate_limited", retryAt}` |
| `desk.askToken` | same viewer | a subscribe-only token for `ask:<jobId>` if the job is theirs | `{error:"forbidden"}` otherwise |

**Panel resolver** (function `panel-fn`).

| golden key | auth rule | work |
|---|---|---|
| `panel.load` | the issue id comes from `context.extension.issue.id`, treated as an unvalidated hint (stated); BROWSE checked for `context.accountId` | KVS `by-issue` + occurrence histogram (bucketed counts kept by `flush`) |
| `panel.applyTriage` | Responder + EDIT_ISSUES on THIS issue (issue-level: security level, reporter-only) | asUser `PUT /issue/{id}` priority; conditional tx (suggestion still current); idempotent |

**Rovo actions.**
- `get-escalation` (GET): input `issueKey` (string). Identity comes from context; the action checks BROWSE and returns the
  stated JSON or `{error}`.
- `ack-escalation` (UPDATE): input `issueKey`. It shares `desk.ack`'s logic.

**JQL function `signalStorming()`.** The golden returns a CONSTANT fragment:
`cf[<signal occurrences field id>] >= 100 AND statusCategory != Done AND issue.property[signal-desk].fingerprint IS NOT EMPTY`.
- It is fresh by construction, because `flush` keeps the field fresh.
- It is user-agnostic, because Jira applies the searcher's permissions.
- It never exceeds the 1,000-value cap.

**Webtrigger `signal-intake`.** §C.8.

### C.4 Rate-tier dosing (what the app must implement; the policy is stated in contract §6)

**Pool.** One 65,000-point pool per virtual UTC hour [DOC tiers#1–4; hard wall BENCH]. It is shared by every
installation in the world and by simulated installations the app cannot see.

**Costs.** The table in `$FORGE_KIT/limits/costs.json` [BENCH, shaped on DOC tiers#5–6]:
- reads: 1 + 1 per core object returned;
- identity and permission objects: 2 each;
- writes: 1 point;
- `search/jql` and `bulkfetch`: 1 + issues returned;
- `permissions/check`: 1 + 1 per 100 ids sent;
- `mypermissions`: 2;
- `app/field/value`: 1 per request;
- `jql/function/computation`: 1;
- bridge `requestJira` from the browser: 0 points (staff-confirmed exemption, stated as an assumption), still burst-bucketed.

**The four stated rules the app must implement** (all graded):
1. **Background cap.** Each installation's background work (anything not started by a person) spends ≤ 12,000 points per
   virtual hour, self-accounted with the table.
2. **Reserve.** When a response's `RateLimit` header reports `global-app-quota` `r` < 6,500 (10%), background work makes no
   further backend Jira call until the reset. Person-initiated work continues.
3. **Per person.** Work started by one person (a resolver or Rovo action, or queued work it started) costs ≤ 600 points per
   virtual hour for that person, however they use the app, including concurrently.
4. **React by reason** [DOC tiers#20]:
   - a quota 429 (`*-quota-*-based`, or an unknown reason) pauses ALL backend Jira calls from that installation until the
     reset;
   - a burst 429 slows only that endpoint (method + path template);
   - a per-issue 429 delays only writes to that issue.

   The next attempt waits for `Retry-After`; if that is absent (the gateway variant), for `X-RateLimit-Reset`; if that is
   absent, for the `RateLimit` `t`.

**How the golden does it.**
- A `gate` module wraps `api.asApp()/asUser().requestJira`.
- Before each call it consults the in-invocation pause state and the KVS pause record (`cfg:pause:<scope>` with a reset
  time).
- After each call it charges the cost to sharded `points` / `personpts` rows. The rows are conditional increments over 8
  shards, summed lazily.
- It parses `RateLimit` / `RateLimit-Policy` as comma-separated, order-free entries, with an optional `r`; `Beta-`
  entries are informational [DOC tiers#18–19]. It records the last pool state in KVS, tenant-keyed.
- On a quota 429 it writes the pause record with the reset time.
- Consumers check the background budget before a batch and defer through `InvocationError` when it is short.

**Spreading.**
- The `heal` hour is split into 12 delayed slices, with a per-installation jitter from the installation id.
- The `sla` sweep reads only KVS.
- The masher (rule 3) is absorbed: the console serves from KVS, and access verdicts are cached ≤ 60 s per (installation,
  person).

**What separates designs, at stated scale.** Measured against the golden on the scoring world, tenant A in the storm
hour; the numbers are estimates to be calibrated:

| workload | naive design | disciplined design |
|---|---|---|
| per-alert status GET + per-occurrence field write | ≈ 3 points per alert, ≈ 96,000 points per storm hour (the pool walls in minutes) | creates + bulk flush + comments + incremental heal ≈ 4,000–6,000 points per hour (≈ 0.15 points per alert) |
| SLA sweep | `search/jql` of every open escalation every 5 minutes ≈ 60,000 points per hour | 0 points (KVS index) |
| console opens by a masher | asUser search per open ≈ 51 points × 200 opens | ≈ 3 points per open with a 60 s verdict cache |

### C.5 Forge LLM features

**1. Background triage** (consumer `triage`, `timeoutSeconds` ≥ 300).
- **Trigger.** One call per new escalation.
- **Prompt content.** Only the escalation's OWN alert titles, labels and a payload excerpt; never other escalations
  (stated content rule).
- **The forced tool `triage`** is stated in the contract:
  - `severity` ∈ {sev1, sev2, sev3, sev4};
  - `category` ∈ {infra, app, data, security, noise};
  - `summary`: a string of 1–280 characters;
  - `duplicates`: an array of ≤ 5 issue keys.
- **Validation.** The app validates every field. Invalid arguments, a refusal (no tool call) or an error give the triage
  state `failed:<reason>`. There is at most one retry per escalation per virtual hour.
- **Applying.** The suggestion is shown with an AI label and is applied to Jira ONLY by a person's click (`applyTriage`,
  EDIT on the issue, asUser).
- **Duplicate suggestions** are shown only if the viewer can browse them.
- **Cache.** The content hash plus model family, so identical inputs make zero new calls.

**2. Ask Signal Desk** (console). `desk.ask` enqueues; consumer `ask` runs the tool loop; the answer is published only
to the asker.
- **Tools** (stated, all read-only, executed with the asker's visibility):
  - `find_escalations({query, state?, limit ≤ 20})`;
  - `get_escalation({issueKey})`;
  - `occurrence_stats({issueKey, windowMinutes})`.
- **The loop.**
  - It answers EVERY tool call, including parallel ones, with `role:'tool'` and a matching `tool_call_id`.
  - It runs at most 4 rounds.
  - An unlisted tool gets an error tool message and is never executed.
- **Grounding.** The shown answer cites only issue keys that the tools returned to this asker in this job; other keys are
  removed.
- **Output** is plain text with an AI label. URLs are never rendered as links.

**3. Rules that apply to both** (stated):
- The model is chosen from a `list()` result at most 10 virtual minutes old, and only `active` ids are used. The list
  changes during a run.
- Sampling rules (documented, stated in §11): never `temperature` and `top_p` together; neither on opus-4-7, opus-4-8,
  opus-5 or sonnet-5.
- Limits: ≤ 100 requests per minute per installation, ≤ 500,000 tokens per minute per model, estimated as
  ceil(chars/4) of the serialised request [DOC llm#20–22, BENCH estimator].
- After an LLM 429 there is no LLM request from that installation for ≥ 60 virtual s (the SDK exposes no Retry-After,
  llm#37).
- `max_completion_tokens` is on every call.
- Budgets:
  - per installation per day in credits, using the tier rates (Haiku 10, Sonnet 30, Opus 50 credits per 1M tokens;
    $0.10 in, $0.50 out per credit), labelled an assumption for unpublished models [DOC llm#30–32];
  - per person, Ask questions per day.

  When a budget is exhausted the app makes 0 calls and shows the state `budget`.
- Error detection by `err.name === 'ForgeLlmAPIError'` or a numeric `err.status` (the class is not exported, llm#36)
  stays discoverable in the SDK source, so it is not restated.

**Latency envelope** (stated):
- a triage call takes up to 40 virtual s;
- an Ask turn takes up to 90 virtual s.

So Ask cannot complete in a 25 s resolver, and opening a surface makes no LLM call.

### C.6 Front-facing Custom UI and its boot budget

**Console** (`jira:globalPage`). Root `[data-testid="desk"]`. Hooks:
- shell `[data-testid="desk-shell"]`;
- ready `[data-testid="desk-ready"]`;
- rows `[data-testid="esc-row"][data-issue-id]`, each with `[data-field="key"]`, `[data-field="occurrences"]`,
  `[data-field="severity"]` and `[data-field="sla"]` (a `<time datetime>` of the next stage due, or the text
  `acknowledged`), plus the button `[data-testid="ack"]`;
- the storm count `[data-testid="storm"]`;
- the paused banner `[data-testid="paused"]` with `<time datetime>`;
- Ask: `[data-testid="ask-input"]`, `[data-testid="ask-submit"]`, `[data-testid="ask-state"]`
  (`pending|done|error|budget`), `[data-testid="ask-answer"][data-ai="true"]` with cited keys as `[data-cite-issue-id]`.

**List semantics.** The open escalations the viewer can browse; the 50 whose next SLA stage is due soonest
(acknowledged last); ties by issue id. It is live within 10 virtual s of a change, with no polling.

**Panel** (`jira:issuePanel`). Root `[data-testid="signal-panel"]`. Hooks: shell, ready, `[data-field="occurrences"]`,
`[data-testid="triage"][data-ai="true"]` with `[data-field="suggested-severity"]` and `[data-testid="apply-triage"]`,
and `[data-testid="notify-step"][data-step]`. On a non-escalation issue it shows `[data-testid="not-escalation"]`.

**Boot budget** (BRIEF §4.3 protocol, "hold-and-release waves", counts and bytes only). The thresholds are proposals,
calibrated on golden and alt, then frozen as numbers in the contract.

| metric | console | panel |
|---|---|---|
| B1 shell at wave 0 (before any backend op resolves) | yes | yes |
| B2 backend waves to READY | ≤ 1 | ≤ 1 |
| B3 backend-bound ops before READY (`invoke`, `requestJira`, …) | ≤ 2 | ≤ 1 |
| B4 app-origin gzip-9 bytes before READY (platform scripts and token CSS excluded) | ≤ 220 KB | ≤ 120 KB |
| B5 app-origin requests; B6 initiator depth | ≤ 8; ≤ 3 | ≤ 6; ≤ 3 |
| B8 bootstrap resolver: sequential backend rounds; calls independent of N (seed N vs 4N escalations) | ≤ 3; identical | ≤ 2; identical |
| B12 shell renders while Realtime and flags are held | yes | yes |
| B13 no repeated bootstrap on in-session navigation; no LLM call while booting | yes | yes |
| B14 bootstrap `invoke` 429 → the shell stays; exactly one retry at or after `rateLimitReset` | yes | yes |

Plus CSP-clean (production `@forge/csp`), no external origins (a benchmark/RoA-style rule, boot#5), theme tokens with
contrast ≥ 4.5:1 in light and dark, and dark surfaces painted with tokens (§F tier V).

### C.7 The UI Kit 2 admin panel (`jira:adminPage`, `render: native`)

`Tabs` (`testId="admin-tabs"`) has six tabs, each `Tab` with a `testId` (Tab delivers it, uikit#31):

| tab | components (allowlisted; testIds only where the wrapper delivers them) | actions → admin API | server-side rule |
|---|---|---|---|
| Integrations | DynamicTable `integrations-table` (rowKey = id); Button `new-integration`; Modal `integration-modal` with Textfield `integration-name`, Select `integration-project`, Button `save-integration`; Button `rotate-<id>`; SectionMessage `secret-once` holding Text `secret-value`; Textfield `relay-token` + Button `save-token` | create / rotate / setRelayToken / delete | Owner; the secret is shown once in the rotating session only |
| Routing | DynamicTable `rules-table`, sortable by order (numeric keys); Modal with Textfield `rule-labels`, Select `rule-project`, Select `rule-severity`; Button `save-rule`; SectionMessage `stale-warning` | saveRule / moveRule / deleteRule with `version` | Owner; optimistic concurrency |
| Responders | DynamicTable `responders-table`; UserPicker labelled `Responder` (stores the `.id` of the onChange object, uikit#29); Button `grant-responder`; Button `revoke-<accountId>` | grant / revoke | Owner |
| Budgets & usage | Text `usage-points-background`, `usage-points-people`, `usage-pool-remaining`, `usage-rejected-quota`, `usage-rejected-burst`, `usage-rejected-issue`, `usage-llm-credits`, `usage-llm-usd`; Textfield `budget-llm-daily`, `budget-ask-person`; Toggle `pause-background`; Button `save-budgets` | setBudgets; usage | Owner |
| Failed work | DynamicTable `failed-table`; Button `replay-<id>` | replayFailed | Owner |
| Migration | Text `migration-progress` (`<done>/<total>`); Lozenge `migration-state` | — | Owner |

**Host contract** (stated, BRIEF §5.4):
- an allowlisted component set; anything else is "unmodelled";
- `Form.onSubmit` is called with no data (use `handleSubmit`);
- uncontrolled Tabs emit no reconcile;
- DynamicTable sorts by cell `key` with the ADS comparator, en-US.

Graded: the ForgeDoc tree, the bridge-call log and the OUTCOMES of the actions (via `admin.state`, signed alerts and
the role effects). Pixels are never graded.

### C.8 Webtrigger and integrations

**Inbound scheme** (stated in full in contract §3; app-implemented, independent of the platform's unshipped
`hmacSharedSecret`, sec#17):
- URL: `<getUrl('signal-intake')>/i/<integrationId>` (the `userPath`). 1.x senders use `?integration=<id>` and keep doing
  so.
- Headers `x-signal-timestamp` (RFC 3339 UTC), `x-signal-delivery` (1–64 of `[A-Za-z0-9_-]`) and
  `x-signal-signature: sha256=<hex>`, where the signature = HMAC-SHA256(secret, `<timestamp>.<deliveryId>.<raw body>`).
  Names are case-insensitive; values arrive as arrays [DOC sec#3 request shape].
- Valid only if |now − timestamp| ≤ 300 virtual s.
- Body `{ "alerts": [ { "alertId", "fingerprint" (1–512 chars), "title" (≤ 300), "labels": {string:string},
  "startsAt", "payload": object (≤ 8 KB) } ] }`: ≤ 100 alerts and ≤ 64 KB per delivery.
- Answers:
  - 202 `{"status":"accepted"}`;
  - 200 `{"status":"duplicate"}` for an already-accepted `deliveryId`;
  - 400 `{"status":"malformed"}`;
  - 401 `{"status":"unauthorized"}` (missing or bad signature, or stale timestamp);
  - 404 for an unknown integration;
  - 503 + `Retry-After` when shedding load.

  The answer must come within 10 virtual s.
- Invalid alerts inside a valid delivery are skipped and listed in Failed work with reason `invalid_alert`.
- The sender (stated) retries non-2xx answers or timeouts with the same `deliveryId` and body and a new timestamp and
  signature. It waits `Retry-After` if given, else 5, 15, 45 … s, for up to 1 virtual hour.
- After rotation both secrets verify for 10 virtual min, then only the new one.

**Outbound "Relay" chat** (benchmark-defined, `$FORGE_KIT/openapi/relay.json`, ~4 KB):
- `POST /v1/rooms/{room}/messages` with `Authorization: Bearer` and `Idempotency-Key` (1–128 characters).
- A repeat with the same key and body within 24 h returns 200 with the original `messageId` and
  `Idempotent-Replayed: true`. The same key with a different body gets 422.
- 429/503 carry `Retry-After`; 401 means a bad token; 410 means the room is archived.
- At most 2 in-flight requests per token.
- "The service may store a message and the connection may close before you see the answer."

Notifications go out on: opened, each SLA stage, acknowledged. Exactly once each.

### C.9 Brownfield: installs upgraded from 1.x (tenant A on every scoring world; `dev-a` on the dev site)

The layout is shipped as `$FORGE_KIT/legacy/signal-desk-1x.json` (~2 KB) and summarised in contract §10:

- **KVS `v1:esc:<sha1(fingerprint)>`** = `{fp, issueKey, count, first, last, open, sev}`.
  - The key is an issue KEY, and some are stale after moves.
  - Some issues were deleted without 1.x noticing.
- **`v1:routes`** = an array of `{match: {label: value}, project, priority}`.
- **`v1:int:<id>`** = `{name, secret}`. These are plaintext secrets (a 1.x defect).
- **`v1:meta`** = `{schema: 1}`.
- **1.x events still queued** on queue `signal-ingest`: `{v:1, integrationId, deliveryId, alerts:[…]}`, with the alerts
  inline.
- **1.x issues** carry the label `signal-desk` and no issue property.
- 1.x counts are history and carry over as they are.

**Guarantees** (contract §10):
- **M1.** An alert for a 1.x fingerprint whose 1.x issue still exists and is open goes to that issue, from the first alert
  after the upgrade, whether or not migration has finished. That needs a dual-read.
- **M2.** Counts continue from the 1.x count.
- **M3.** 1.x events are processed exactly once.
- **M4.** 1.x secrets keep verifying, and after migration no secret is readable through `kvs.get` or `kvs.query`.
- **M5.** Within 60 virtual minutes every open 1.x escalation issue carries the `signal-desk` property, and migration is
  finished. It resumes after any interruption, never uses a stored cursor, and stays within the background cap.
- **M6.** A 1.x escalation whose issue was deleted opens a new escalation on its next alert.

**Upgrade delivery** (stated): the upgrade may come with or without `avi:forge:upgraded:app` (major-only, DOC rob
§10/lifecycle), and before or after alerts start flowing.

### C.10 Alternative designs that must also pass (they prove the guarantees are outcome-only)

| guarantee | golden | also valid |
|---|---|---|
| exact counts | idempotent occurrence rows + recount | per-escalation counters with CES conditional increments and a per-delivery checkpoint; partition keys (`concurrency.key` = fingerprint hash mod k) with read-modify-write |
| one issue per fingerprint | `fpstate` reservation + token property + a lookup after the stated search lag | the same reservation with `reconcileIssues` (≤ 50 ids) once an id is known; a per-fingerprint serial queue key plus the token lookup |
| JQL freshness | constant custom-field fragment | issue-property fragment kept by `flush`; an id list plus computation updates (fails only above 1,000 matches, which tenant A has) |
| console list | KVS by project + bulk `permissions/check` | bridge `requestJira` search as the user (points-exempt) for ids, plus a resolver that re-checks every id it returns |
| Ask visibility | asApp + `permissions/check` with the asker's `accountId` | `asUser()` in the consumer IF the fidelity probe shows it works for person-started queued work (I-12); the contract states the measured rule |
| ingest | one event per delivery | KVS inbox + doorbell; webtrigger backpressure (503 + Retry-After) under the event budget |

---------------------------------------------------------------------------------------------------------------------

## D. The world

### D.1 The guarantees (what contract §1–§10 state; each maps to rows in §F)

| id | guarantee (abridged; the contract wording is in §E) |
|---|---|
| G1 | One open escalation issue per fingerprint, whatever happens to deliveries, invocations and Jira's answers. |
| G2 | Each accepted alert counts exactly once, plus 1.x history. |
| G3 | Reflected in the `Signal occurrences` field and in every surface within 3 virtual min of acceptance (within 5 virtual min after Jira stops refusing the app). |
| G4 | Every delivery is accepted within 2 virtual min of its first send, within the stated sender limits; the webtrigger answers in ≤ 10 virtual s. |
| G5 | A delivery is processed once however many times it arrives, including simultaneously; unsigned, forged, tampered or stale requests have no effect. |
| G6 | Routing by the rules in force at acceptance; severity sets priority; the `signal-desk` property is set at creation. |
| G7 | Storm comments at 100 and 1,000 occurrences, once each. SLA stage comments and notifications once each, within 6 virtual min of the stage time, never after acknowledgment. |
| G8 | Chat: exactly one message per (escalation, step). |
| G9 | Resolved: a new escalation for alerts accepted later (≥ 1 virtual h after resolution if Jira's event is lost [J window]). Deleted: a new escalation by the next write. Moved: the same escalation, new key shown. |
| G10 | A person sees only escalations whose issues they can browse, in every surface, response, realtime message and AI answer; access decisions are reused ≤ 60 virtual s. |
| G11 | Owners = Jira administrators (live); Responders granted by Owners; refusal `{error:"forbidden"}`, no effect; payload identity ignored; `context.extension` ids are hints. |
| G12 | Secrets: stored in the secret store, never returned (except one reveal on create/rotate), logged, sent anywhere but their destination, or put in prompts. |
| G13 | Tier 1: background cap 12,000 per installation-hour; reserve below `r` < 6,500; ≤ 600 per person-hour; react by reason; retry timing. |
| G14 | Scheduled work is spread: no virtual minute carries > 15% of an installation's scheduled points in an hour. |
| G15 | AI writes nothing without a person's click; forced-tool output is validated; Ask grounding; budgets; model freshness; LLM rate rules; latency placement; failure states. |
| G16 | Admin saves never silently overwrite a newer version (`stale`). |
| G17 | Boot budgets (§C.6) and look-and-feel rules. |
| G18 | 1.x upgrade M1–M6. |
| G19 | No feedback loops on the app's own Jira changes. |

### D.2 Platform facts the app lives under (contract §11; this list, not a fault list, is what is "announced")

Each fact is a property of the world. The marker says where its truth comes from.

| fact | source |
|---|---|
| Async events: at least once, any order, retried within 24 h; `InvocationError.retryAfter` ≤ 900; `jobId` is per push; 500 events per minute per installation shared by all queues, 50 per push, 200 KB per push, 100 KB per event when timeout > 55 s; cyclic limit 1,000; concurrency keys per installation across queues | DOC rob#15–30 |
| Without a concurrency key, up to 16 invocations of a consumer may run at once per installation | BENCH (docs: "unbounded", rob#28) |
| Timeouts: resolver 25 s; consumer and scheduled 55 s default, up to 900; webtrigger and action 55 s. At the timeout the invocation stops; writes it made persist | DOC rob#1–4; kill semantics stated |
| Warm processes are reused across installations; module state may persist or reset at any time; unawaited work may run later inside another installation's invocation, or never | DOC rob#11–12 |
| KVS `get` strict; query eventually consistent (results may miss writes from the last 5 virtual s); last write wins; `FAIL_IF_EXISTS` → 409 `KEY_CONFLICT`; CES transactions ≤ 25 ops with conditions (false → 400 `CONDITIONAL_CHECK_FAILED`); no `keyPolicy` in batch or transact; cursors valid only in the invocation that produced them; 1,000 RPS, 4,000 + 4,000 10 KB units per minute, 240 KiB values | DOC rob#43–59; MEAS (codes); BENCH (lag 5 s; cursor scope) |
| Jira: three limit systems; the cost table; burst buckets per tenant × endpoint × method (GET/POST 100, PUT/DELETE 50 rps, capacity = 1 s of steady state, shared with other traffic on the tenant); per-issue writes 20 per 2 s and 100 per 30 s, counting edits, comments, property writes and app-field writes; header grammar (`r` only past ~80%, `Beta-` informational, order-free, ISO with or without seconds); gateway 429 without Retry-After; an unknown reason is quota-class; quota = hard wall to the top of the hour; your app's other installations spend from the same pool | DOC tiers#1–20; BENCH (costs, capacity, write counting, hard wall, phantom) |
| `@forge/bridge` `requestJira` from the browser: not charged points and not blocked by the hourly pool; burst-limited | staff-confirmed (tiers#24,25) → stated assumption |
| Jira may commit a write and still lose its answer (a 5xx, or a thrown `ProxyRequestError` / network error) | BENCH (a general HTTP fact; probe I-7) |
| Some Jira and Relay calls take much longer than usual, up to 120 virtual s | BENCH |
| `search/jql` may not show issues created or changed in the last 30 virtual s unless `reconcileIssues` (≤ 50 ids) names them | OAS + BENCH (magnitude) |
| Product events may be late (≤ 3 virtual min), duplicated, out of order or lost; deletes are emitted for top-level entities only; without `ignoreSelf` your own changes come back as events | DOC rob#33–36; BENCH (loss) |
| A fresh installation's app user may get 401/403 for up to 20 virtual min | DOC rob#37; BENCH (window) |
| Scheduled triggers: per-installation offsets; duplicate, skipped or overlapping runs happen; a throw is not retried | DOC rob#38–41; BENCH (skips, overlaps) |
| Realtime: 50 ops/s per installation (publish returns `errors[]`); `publish` from queued work is unsupported (use `publishGlobal`); `publishGlobal` reaches `subscribeGlobal` on the same channel unless tokens carry different claims; publish-only and subscribe-only tokens; `subscribe()` is module-context scoped | DOC rob#63, [B], CHANGE-3326; probe I-9 |
| Forge LLM: `chat`/`stream`/`list`; `tools` + `tool_choice` (`required` or a named function forces a call, BENCH); no structured outputs; sampling rules; 100 RPM, 500k TPM per model; streams can end without `finish_reason`; the model list changes; error shapes in `BENCHMARK-VALUES` | DOC llm#1–38; BENCH (error shapes, forcing) |
| Issues can be resolved, moved (new key, same id) or deleted by people at any time; people gain and lose access at any time; another Owner may edit the same rule | world fact |
| Installs upgraded from 1.x: §10 | stated |

### D.3 Scoring worlds (three seeded worlds; per-row WORST for correctness, MEAN for economy)

Each world runs **three installations on one global pool**, plus simulated "other installations". The contract states
upper bounds only ("up to 3 installations at once; ≤ 40,000 issues and ≤ 8,000 escalations per installation; storms up
to the sender limits for ≤ 15 virtual minutes; ≤ 3,000 new fingerprints per installation-hour; ≤ 4,000 open 1.x
escalations"). The table below is NOT in the contract.

| | tenant A (upgraded from 1.x) | tenant B (fresh) | tenant C (small, interactive) |
|---|---|---|---|
| projects | 8 (one with an issue-security scheme; one restricted to a group) | 4 | 2 |
| issues | ~30,000 (6,000 from 1.x: 3,000 open, 3,000 resolved; ~1,100 open with 1.x count ≥ 100) | ~10,000 | ~2,000 |
| users | ~60: 3 Owners (one demoted mid-run), 15 Responders, 2 security-level-restricted, 1 reporter-only-edit, 1 losing project access mid-run | ~20 | ~12 |
| integrations | 3 from 1.x (plaintext secrets, `?integration=` URLs) + 1 created by the probe | 2 (probe) | 1 (probe; no Relay room) |
| alerts | ~40,000: a storm of ~32,000 over 8 virtual min (~4,000 per minute) across ~2,500 new fingerprints; 3 hotspots of 1,500 occurrences in 90 s; the rest steady | ~8,000 (steady + a 3,000-alert mini storm) | ~1,000 |

**Timeline classes** (seeded times; the contract never names them):
- **Install and upgrade.** B has a 15-virtual-minute 403 window. A's upgrade sometimes comes without the upgrade event,
  with 1.x events queued.
- **Storms** with retried and concurrent duplicate deliveries.
- **Lost answers on writes.** A seeded set of fingerprints, escalations and steps loses the answer to its first create,
  comment, chat or field write.
- **Slow calls.** A seeded set gets 40–120 virtual s, so consumers are killed at their timeouts mid-work.
- **Product events** duplicated, delayed, reordered and lost.
- **World changes:**
  - ~40 resolutions (6 with lost events);
  - ~12 moves (2 with lost events);
  - ~10 deletions (3 with lost events);
  - a project-access loss;
  - an Owner demoted;
  - two Owners editing the same rule mid-storm;
  - a secret rotation while deliveries are in flight;
  - the model the app called most in the first hour marked `deprecated` (implementation-agnostic: every app meets its
    own model's deprecation).
- **Phantom spend.**
  - A slow ramp in hour 2 pushes the pool past 80% and then below the reserve unless the app yields (it never walls a
    compliant app).
  - A sudden phantom wall in hour 3 (~20 virtual min, Retry-After up to 1,200 s) every app must survive. It is not
    attributed to the app.
- **One gateway-variant 429 window**, one burst 429 window on one endpoint, and per-issue 429s wherever an app writes too
  fast.
- **Interactive load:**
  - console opens by 5 viewers every ~2 virtual min;
  - 3 Responders acknowledging, including a double click, two tabs and two people on one escalation;
  - 10 Ask questions, 2 of them carrying injected instructions;
  - one masher: 300 invokes in 3 virtual min at concurrency 10.
- **Scripted LLM failure mix**, keyed by fingerprint hash: ~85% clean, 4% refusal, 3% malformed arguments, 2%
  "obey the injection", 2% 500, 1% 403 `FORGE_LLMS_MODEL_FORBIDDEN`, 1% 429, 2% truncated stream / empty `end_turn`.
- **Rerun.** An idle hour at the end must produce zero writes.

### D.4 The dev site (fairness invariant: every graded class occurs on it)

- **Tenants.** Two: `dev-a` (upgraded from 1.x, 300 1.x escalations including moved and deleted ones, plaintext
  secrets) and `dev-b` (fresh, 5-minute 403 window).
- **Size.** 2,500 issues and 10 users (Owner, Responders, a security-level-restricted viewer, a reporter-only editor, a
  user who loses access).
- **`forge-dev scenario`.** Runs a 1-virtual-hour mini world with every fault class above at least once, at seeded times,
  and prints the SENDER's log (alerts sent and accepted per fingerprint), which any real developer would have. It prints
  no judgment of the app.
- **Fault controls.** `forge-dev storm` (rate, size, duplication, concurrency) and `forge-dev world`
  (resolve/move/delete/revoke/demote/rotate/deprecate-model/phantom/wall/gateway-429) trigger any class on demand.
- **Inspection.** `forge-dev jira <method> <path> --as <user>` is a read-only REST client as a person. `forge-dev usage`
  is the gateway ledger: points by installation, class and person; rejections by reason; KVS units; events per minute;
  Realtime ops; LLM RPM, TPM and credits. It plays the developer-console role staff recommend as a "test quota" layer
  (tiers#48).
- **Other tools.** `forge-dev clock advance <d>`; `forge-dev serve <module> [--as]`, which serves Custom UI with the real
  bridge and the UI Kit admin page in the offline host; `kvs`, `users`, `reset`.
- **Silent semantics.** The dev site prints platform errors exactly as production would (lint, 4xx/5xx, 409, 429). It
  prints NOTHING about semantic outcomes: no "duplicate escalation", no "count mismatch".

### D.5 What is not stated, and why that is fair

Not stated:
- the scenario timeline and sizes beyond the bounds;
- which entities lose answers or get slow calls;
- that a phantom wall happens in hour 3;
- that the most-used model is the one deprecated;
- that faults coincide.

All of these are INSTANCES of stated facts. An app that honours every fact on every code path passes regardless of
timing; that is what "guarantee, not trap list" means. Fairness is argued in full in §H.

---------------------------------------------------------------------------------------------------------------------

## E. Contract outline (`forge2/public/`)

Size budget: about 29 KB of core public input:
- `spec-build-forge2.md` ≈ 3.2 KB;
- `SIGNAL-CONTRACT.md` ≈ 21.5 KB;
- `STARTER.md` ≈ 4.3 KB.

On-demand reference files of about 10 KB sit in the kit and are looked up, not read whole:
- `limits/costs.json` 2.0;
- `openapi/relay.json` 4.0;
- `legacy/signal-desk-1x.json` 2.0;
- `BENCHMARK-VALUES.md` 2.5 (error shapes, latency model, header samples).

That is above 1.0's 18.9 KB (the target was ≤ 25 KB) and far below the 45.7–70 KB that produced desk audits; see K-2.

| § | title (KB) | key guarantee sentences (verbatim drafts) |
|---|---|---|
| prompt | Definition of done, budget, bands (3.2) | "You have a budget of 250 model calls." "The scoring sites are bigger and harsher than the dev site: three installations at once on one rate-limit pool, storms, and a world that changes while your app runs. The dev kit reproduces every condition they have. Nothing will tell you when your app is wrong except your own checks." The band paragraph (§F.1) in words. |
| 1 | Escalations (3.0) | "A fingerprint has at most one open escalation: one Jira issue created by your app, carrying the issue property `signal-desk` = `{"fingerprint": …, "integration": …}`. Whatever happens to deliveries, invocations and Jira's answers, your app never creates a second issue for a fingerprint whose escalation is open." / "An escalation's occurrences are the distinct alerts (by `alertId`) your app accepted for its fingerprint since it opened, plus its 1.x count. Each accepted alert counts exactly once." / "Within 3 virtual minutes of accepting an alert, the `Signal occurrences` field and every surface show it; while Jira refuses your app, within 5 virtual minutes after it stops." / Routing, severity → priority, storm comments (`Storm: <N> occurrences` at 100 and 1,000, once each), SLA table (sev1 5/15, sev2 15/60, sev3 60/240 min, sev4 none; once each, within 6 virtual min, never after acknowledgment), resolve/delete/move outcomes. |
| 2 | Modules, scopes, egress (1.6) | Module table (B). "Request only the scopes your calls need (the OpenAPI rule of 1.0)." "Egress: `https://relay.signal-desk.example` only." "Runs on Atlassian is not required." |
| 3 | Inbound alerts (2.2) | The scheme of §C.8 in full. "A delivery is processed exactly once however many times it arrives, including at the same time." "Requests that fail verification change nothing." "Answer within 10 virtual seconds; a slower answer counts as a failure and the sender retries." "Within the sender limits, every delivery is accepted within 2 virtual minutes of its first send." |
| 4 | Chat notifications (0.8) | "Post to the integration's Relay room when an escalation opens, at each SLA stage, and when it is acknowledged — exactly once each." Pointer to `relay.json`. |
| 5 | People, roles, visibility (2.0) | "Resolvers can be invoked directly by anyone who can load the surface, with any payload. Identity comes only from the invocation context; `context.extension` ids are hints, not proof of access; payload fields such as `accountId`, `role` or `isAdmin` are ignored." / "Owners are the site's Jira administrators, checked against Jira. Responders are granted by Owners." / "A person sees — in any surface, response, realtime message or AI answer — only escalations whose issues they can browse. Hidden escalations are omitted, not redacted." / "An access decision may be reused for at most 60 virtual seconds." / "Refusal: `{ "error": "forbidden" }` and no other effect." / Secrets rule. |
| 6 | Budgets: Tier 1 and the platform (2.6) | "Your app is on Tier 1: one 65,000-point pool per virtual UTC hour shared by every installation, including ones you cannot see. When it is exhausted every backend Jira call from every installation is refused until the top of the hour." / the four rules of §C.4 / "Work started by a person (a resolver or Rovo action, or queued work it started) counts against that person; everything else is background." / "Scheduled work is spread: no virtual minute carries more than 15% of an installation's scheduled points in an hour." / "Do not let a function wait for a limit to clear." |
| 7 | Forge LLM (2.4) | The two tool schemas, the content rule, "AI output is untrusted: it changes nothing in Jira until a person clicks Apply", the loop bound, "answer every tool call", grounding, labels, model freshness, the rate and budget rules, failure states, the latency envelope ("a triage call takes up to 40, an Ask turn up to 90 virtual seconds"), "opening a page makes no LLM call". |
| 8 | Surfaces (3.4) | 8.1 console hooks and list semantics; 8.2 panel hooks; 8.3 admin panel tabs and testIds, plus the admin API table; 8.4 boot budgets (the numbers of §C.6) and "boot is graded cold, by counting, never by time"; 8.5 look and feel (tokens, dark mode, contrast, "text from alerts and AI is shown as text; links from alerts are never clickable"). |
| 9 | Rovo and JQL (0.9) | The two actions' inputs and JSON; the skill directory and frontmatter rules; the agent; mcp `name` ≤ 30. "`issue in signalStorming()` returns the open escalations with ≥ 100 occurrences, the same for every person (Jira shares JQL-function results across people until your app updates them; a fragment may hold at most 1,000 values), fresh within 2 virtual minutes." |
| 10 | Installs upgraded from 1.x (0.9) | Pointer to the layout; M1–M6 verbatim. |
| 11 | How the platform behaves here (2.4) | The D.2 list, compact, each fact with "documented" or "benchmark value". |
| 12 | What the harness does differently (0.9) | Virtual time (the clock advances by a stated latency model; waits cost virtual time; CPU is free); fresh-versus-warm processes simulated; no real Jira; flags drawn outside your frame; UI Kit host allowlist; no pixels graded; the platform webtrigger HMAC not implemented (declaring it changes nothing); scoring worlds bigger than dev. |

---------------------------------------------------------------------------------------------------------------------

## F. Check registry

### F.1 Composition, criticals, bands, excellence

**Composition.** `earned = (0.90 · inner + 0.10 · gate · e_mean) × crit_mult`, and
`final = min(earned, ceiling − 0.05·(1 − earned))`. BAND_PULL is unchanged from 1.0.
- `inner` = the weighted mean of tiers.
- Tier means are plain means of their non-diagnostic, available, non-vacuous rows.
- `unavailable` makes a verdict unpublishable.
- `vacuous_root` prices 0 and fires no multiplier.

**Tier weights** (sum 1.00):

| L lint | K currency | I ingest & exactly-once | T time & continuation | W changing world | D Tier-1 dosing | S security | A Forge LLM | U UI function | B boot | V visual | M migration | R Rovo |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| .04 | .06 | .15 | .08 | .08 | .13 | .14 | .08 | .09 | .04 | .03 | .06 | .02 |

**Criticals** (factor = 0.6 + 0.4 · severity input).

Rules:
- Each fires ONLY on the observed consequence it names.
- Each root is priced once.
- The product of all factors is floored at 0.36. This keeps spread for mid models; inner and bands still separate them.

| critical | named consequence (must be OBSERVED) | severity input |
|---|---|---|
| `l_deployable` | the app does not deploy (client lint, the measured server-rule pack, or the pinned runtime flag) | 0 on any error |
| `l_bundles_load` | crash: functions do not load | 0 if < 0.4 of functions load, else 1 |
| `i_one_escalation` | a fingerprint had two open escalation issues at once | 1 − min(1, duplicated fingerprints / 3) |
| `i_no_lost_alerts` | accepted (2xx) alerts never reflected | min(s, 0.5) when s < 1 (the SB data-loss transform) |
| `s_no_leak` | a person received hidden data | 0 |
| `s_admin_authz` | a non-Owner's (or a demoted Owner's, after 60 s) admin call changed state | 0 |
| `s_secret_store` | a secret canary surfaced (response, log, egress, prompt). Storage-hygiene failures alone do not fire it | 0 |
| `s_tenant_isolation` | one installation's canary reached another | 0 |
| `d_no_self_wall` | the pool hit 0 in an hour where the app made background calls after seeing `r` < 6,500 | 0.5 |
| `u_console_loads` | the console never reaches READY | 0 if never |

**Admission bands.** A row is passed only when it is exactly 1.0, available and not vacuous.

| band | max | requires |
|---|---:|---|
| 1 deployable | 0.499 | `l_deployable`, `l_bundles_load` |
| 2 working desk | 0.649 | `u_console_loads`, `x_escalations_exist`, `x_admin_renders`, `x_panel_renders` |
| 3 complete current surfaces | 0.799 | every K row; `u_console_list`, `u_panel`, `u_admin_integrations`, `u_admin_rules`, `u_admin_roles`, `a_triage_valid`, `a_ask_loop`, `r_get_escalation`, `v_dark` |
| 4 production robustness (graded) | `max(0.55, 0.949 − 0.025·(n − 1))` for n ≥ 1 DEFECTS | roster: every I, T, W, D, S, A (not in band 3) and M row; `u_console_live`, `u_ack`, `u_paused`, `u_apply_triage`, `u_admin_usage_failed`; `b_shell`, `b_one_wave`, `b_429` |

How band 4 counts:
- n counts DEFECTS (roots, via ROOT_BLOCKS), not rows.
- Absent or vacuous roster rows are failures, but are priced once through their root.

Band 4's cap is graded all the way down to 0.55. 1.0 floored it at band 3's 0.799, and that compression is why every
complete 1.0 app landed between 0.8 and 0.9. Dominance still holds: every cap applies to every app, so dropping a
surface never escapes its robustness defects.

**Excellence** (0.10 slice). The gate is proportional: the fraction of {`i_one_escalation`, `i_exact_counts`,
`s_no_leak`, `u_console_list`, `d_background_cap`, every K row} at 1.0. The E rows are continuous,
`min(1, golden_worst_of_5 / app)`, and each is a MEAN over the three worlds. This repairs 1.0 defect F (no step rungs).

| E row | measured |
|---|---|
| `e_points_per_alert` | gateway points of background work ÷ accepted alerts (storm hour, all tenants) |
| `e_points_per_open` | points attributable to console opens ÷ opens |
| `e_kvs_units_per_alert` | KVS 10 KB write units ÷ accepted alerts |
| `e_compute_per_alert` | Σ invocation virtual duration × memoryMB/1024 (GB-s) ÷ accepted alerts. In-function sleeping shows here, which matches billed reality (tiers#44) |
| `e_llm_tokens_per_escalation` | LLM tokens ÷ new escalations |
| `e_realtime_ops_per_change` | Realtime ops ÷ escalation changes |

The contract's prompt states the golden's measured values as targets ("the reference spends about 0.15 points per
accepted alert"). That follows the frontier report: "If economy matters, state it."

### F.2 Rows

Columns: id · what is measured · contract anchor · weight · C (critical) · load-robust measurement.

How the scorer measures, every row:
- Every measurement is a count, a virtual-time quantity, a state diff, a ForgeDoc tree or a CDP count. None is wall-clock.
- The proxy and gateway logs keep page summaries, not bodies (machinery N9).
- Leak scans use whole-token matching (1.0 G6).

**L — deploy and lint (0.04)**

| id | measured | anchor | w | C | measurement |
|---|---|---|---:|:-:|---|
| `l_deployable` | 0 lint errors from the CLI's client half + the pinned `deprecated-runtimes` flag + the measured server-rule pack (MANIFEST_INVALID_RULE texts); lint ×2 identical | P done 1; §2 | .012 | C | kit lint (byte-identical client packages, MEAS) |
| `l_bundles_load` | share of manifest functions that bundle (esbuild as deploy) and load; every schema-valid consumer form is invocable (1.0 defect C) | P done 1 | .012 | C | build + wrapper load |
| `l_lint_warnings` | 1 − 0.2·distinct warnings | P done 1 | .006 | | lint |
| `l_real_packages` | `@forge/*` at kit pins; no imports outside the kit; no `react-dom` in the UI Kit bundle | STARTER | .004 | | esbuild metafile |
| `l_scopes_egress` | scopes = the per-call OpenAPI rule + `storage:app` (`read/write:app-data:jira` optional); egress = exactly the Relay host; 1 − 0.25 per missing or extra | §2 | .006 | | lint + call log |

**K — platform currency and wiring (0.06; 9 rows × .00667)**

| id | measured | anchor | measurement |
|---|---|---|---|
| `k_modules_current` | no `jira:fullPage`, `jira:dashboardGadget`, `@forge/ui`, `@forge/api` `storage`, `nodejs20.x`, `/rest/api/3/search`; runtime `nodejs22.x`/`24.x` | §2 | AST + manifest + log |
| `k_front_surfaces` | one `jira:globalPage` and one `jira:issuePanel`, both Custom UI resources | §2, §8 | manifest |
| `k_admin_native` | `jira:adminPage` `render: native`; the resource rules (file or stated bundler); the ForgeDoc root renders | §2, §8.3 | manifest + host |
| `k_custom_field` | `jira:customField` `type: number`, `readOnly: true`; values written through the app-field API | §1, §2 | manifest + log |
| `k_jql_function` | `jira:jqlFunction` `signalStorming` with a function; returns `{jql}` that the site parses | §9 | manifest + site |
| `k_llm_model_current` | one `llm` module (`claude`); every call names an id `list()` reported `active` ≤ 10 virtual min before | §7 | LLM log |
| `k_webtrigger` | the `signal-intake` webtrigger with a function; every response carries `statusCode` | §3 | manifest + ingress log |
| `k_triggers` | issue updated and deleted triggers; lifecycle installed and upgraded | §2 | manifest |
| `k_rovo_wiring` | both actions (inputs, `actionVerb`), `rovo:agent` lists the skill, `rovo:skill` frontmatter (name = dir, description 50–1,024, `allowed-tools` ⊇ both), `rovo:mcp` `name` ≤ 30 exposing `get-escalation` | §9 | manifest + tree |

**I — ingest and exactly-once (0.15; 15 rows × .01)**

| id | measured | anchor | C | measurement |
|---|---|---|:-:|---|
| `i_auth_reject` | unsigned, wrong signature, body changed after signing, timestamp outside ±300 s, unknown integration → the stated status; ZERO effects (KVS diff, pushes, Jira, Relay, logs) | §3 | | crafted requests; effect log until +10 virtual min |
| `i_auth_accept` | header-name case variants and multi-valued arrays accepted; both secrets verify during the 10-minute grace; the old one gets 401 after | §3 | | crafted requests |
| `i_delivery_once` | sequential re-sends and concurrent pairs of one `deliveryId` → processed once (occurrence and count effects); 202 then 200 `duplicate` | §3 | | oracle on crafted deliveries |
| `i_answer_fast` | every webtrigger invocation's virtual duration ≤ 10 s | §3 | | virtual clock |
| `i_accept_complete` | share of deliveries accepted within 2 virtual min of first send (within the sender limits) | §3 | | sender log |
| `i_one_escalation` | share of fingerprints never having two open escalation issues | §1 | C | Jira state at every checkpoint; issues mapped by the `signal-desk` property |
| `i_no_lost_alerts` | share of accepted alerts reflected at settle (UNDER-count side) | §1 | C | field + surfaces vs sender log |
| `i_exact_counts` | escalations whose field = console = panel = Rovo = oracle (OVER-count and disagreement side) | §1 | | oracle |
| `i_reflect_3min` | share of (fingerprint, minute) checkpoints with field(t) ≥ accepted(t − 3 min), outside refusal windows (+5 min after) — THE THROUGHPUT BAR | §1 | | per-virtual-minute snapshots |
| `i_routing` | project and priority = the rule in force at the first alert's acceptance; the property set at creation | §1 | | oracle + create log |
| `i_storm_comments` | one `Storm: <N> occurrences` ADF comment per crossing (100, 1,000); none extra | §1 | | site comments |
| `i_no_dup_writes` | no app-authored comment (storm, SLA, acknowledgment) duplicated under lost answers and redelivery; idempotent writes converge | §1, §11 | | site comments + write log |
| `i_notify_once` | one Relay message per (escalation, step) for configured integrations; none for unconfigured; stable `Idempotency-Key` per step | §4 | | Relay store + request log |
| `i_sla` | stage comments and notifications once per stage, within 6 virtual min of the stage time, none after acknowledgment | §1 | | oracle |
| `i_poison` | invalid alerts listed in Failed work (`invalid_alert`), the rest processed; a Relay 410 failure listed, and after the Owner fixes the room, replay delivers once | §3, §8.3 | | `admin.state` + Relay store |

**T — time and continuation (0.08; 8 × .01)**

| id | measured | anchor | measurement |
|---|---|---|---|
| `t_kill_resume` | entities touched by invocations killed at their timeout end correct (no loss, no double count, no extra issue or comment) | §11 | oracle over the kill set |
| `t_wall_defer` | work deferred while Jira refused the app (install window, phantom wall) completes ≤ 5 virtual min after the refusal ends; nothing lost | §1, §6 | snapshots |
| `t_no_wait_in_function` | invocations idling > 30 virtual s on a timer after a 429 = 0 | §6 | in-child time agent |
| `t_retry_timing` | every repeat of a refused request comes ≥ Retry-After (else `X-RateLimit-Reset`, else `t`) after the refusal; one definition of "repeat" (1.0 §17.6) | §6 | gateway log |
| `t_schedule_anomalies` | duplicate, overlapping and skipped scheduled runs → no double SLA effect, no missed stage beyond tolerance, heal not doubled | §11 | oracle + write log |
| `t_unawaited` | proxied calls issued by continuations resumed after their invocation returned = 0 | §11 | emulator freeze-at-return |
| `t_events_disorder` | duplicated, delayed, reordered or lost product events → final escalation states correct by settle (lost: within the 1-hour heal window) | §11 | oracle |
| `t_push_errors` | work in pushes that failed (RateLimit, PartialSuccess, PayloadTooBig, InvocationLimit) is completed later | §11 | push log + oracle |

**W — changing world (0.08; 8 × .01)**

| id | measured | anchor | measurement |
|---|---|---|---|
| `w_moved` | a moved escalation continues (same id); surfaces show the new key; no new escalation | §1 | oracle |
| `w_deleted` | a deleted issue → a new escalation by the next write; failing invocations on the 404 ≤ 3 | §1 | oracle + invocation log |
| `w_resolved` | alerts accepted after the app learned of a resolution (or > 1 virtual h after it) open a new escalation; SLA stops [J window] | §1 | oracle |
| `w_rule_change` | concurrent rule edits mid-storm: alerts accepted after the committed save follow it | §1, §8.3 | oracle |
| `w_access_change` | 60 s after a person loses project access, an Owner loses ADMINISTER, or a Responder is revoked: nothing new reaches them, and no admin or ack effect | §5 | probes at +61 virtual s |
| `w_model_deprecated` | no call names a model > 10 virtual min after it became `deprecated` | §7 | LLM log |
| `w_jql_fresh` | `signalStorming()` as two people at checkpoints = oracle within 2 virtual min, including with > 1,000 matches | §9 | site search |
| `w_self_events` | Jira or queue work caused by the app's own changes = 0; writes per escalation bounded | §11 | lineage of events |

**D — Tier-1 dosing and platform limits (0.13; 12 × .01083)**

| id | measured | anchor | C | measurement |
|---|---|---|:-:|---|
| `d_background_cap` | per installation-hour background points ≤ 12,000; score = min over hours of max(0, 1 − 2·overshoot share) | §6 | | gateway ledger |
| `d_reserve` | background Jira calls after the app received `r` < 6,500, until the reset = 0 | §6 | | gateway log |
| `d_no_self_wall` | (critical-only row) the pool never hit 0 after the app's own reserve violations | §6 | C | gateway log |
| `d_quota_pause` | after a quota 429, backend Jira calls from that installation before the reset = 0 | §6 | | gateway log |
| `d_burst_scope` | after a burst 429 on E: no early repeat on E; other endpoints with pending work resume within Retry-After + 5 virtual s | §6 | | gateway log + oracle of pending work |
| `d_per_issue` | per-issue 429s caused by the app = 0 (else 1 − rejected/writes × 10); after one, only that issue waits | §6 | | gateway log |
| `d_per_person` | the masher's attributed points ≤ 600 per virtual hour (continuous above it) | §6 | | lineage-attributed ledger |
| `d_interactive` | person actions succeed, or show `paused` correctly during app-wide refusal, throughout (including during other tenants' storms) | §6 | | interactive script results |
| `d_scheduled_spread` | no virtual minute carries > 15% of an installation's scheduled-origin points in an hour | §6 | | ledger by minute |
| `d_headers` | `admin.usage.pool` matches the last headers the app received (multi-policy, `Beta-`, optional `r`, both ISO forms) | §6, §8.3 | | fuzzed headers on the dev and scoring sites |
| `d_platform_limits` | app-caused platform-limit errors: async (events per minute, per push), KVS units and RPS, Realtime ops/s, bridge limiter; each class equally | §11 | | counters |
| `d_llm_rate` | ≤ 100 RPM per installation, ≤ 500k TPM per model (stated estimator); ≥ 60 virtual s without LLM calls after an LLM 429 | §7 | | LLM log |

**S — security (0.14; 14 × .01)**

| id | measured | anchor | C | measurement |
|---|---|---|:-:|---|
| `s_admin_authz` | every stated admin key and every key the admin panel invoked, replayed as a Responder, a browse-only user, and a demoted Owner (after 60 s) → `forbidden`, zero diff (KVS, Jira, Relay, secrets) | §5, §8.3 | C | replay + diff |
| `s_role_escalation` | non-Owner grants (including self); forged `role`, `isAdmin`, `accountId` → refused or ignored | §5 | | replay |
| `s_identity` | acknowledgment records and mentions attributed to `context.accountId` even with forged payload ids | §5 | | site comments |
| `s_no_leak` | hidden canaries (keys, summaries, alert titles of hidden escalations) absent from DOM, resolver and action responses, realtime deliveries to that person, person-facing prompts, and Ask answers | §5 | C | scans |
| `s_idor` | panel and console calls with ids the person cannot browse (including a forged extension) → omission or refusal | §5 | | replay with substituted ids |
| `s_issue_perms` | apply-triage and acknowledge refused where only a project-level grant exists (security level, reporter-only) | §5 | | probe |
| `s_secret_store` | integration secrets and Relay tokens in `setSecret`; never readable through `kvs.get`/`query`; read-back is presence only; the one-time reveal goes only to the rotating Owner's session; never in logs, egress (except the bearer to Relay) or prompts | §3–§5 | C (exposure only) | KVS dump + scans |
| `s_xss` | canary markup in titles, labels, payloads and LLM output is inert: `window.__xss` unset; no `javascript:` hrefs; alert links not clickable; no `unsafe-inline` scripts | §8.5 | | DOM + CSP log |
| `s_tenant_isolation` | A's canaries never in B or C responses, storage, realtime or logs, under warm reuse | §11 | C | scans |
| `s_realtime` | tokens minted server-side with server-derived claims; another person subscribing to someone's Ask or console channel gets nothing; multi-person payloads carry only opaque ids | §5, §8 | | realtime log |
| `s_llm_injection` | a scripted model that "obeys" the injection (unlisted tool, write tool, hidden key, URL) → no Jira write without a click, no unlisted tool executed, no egress, hidden key not shown, URL not a link | §7 | | LLM log + write log + DOM |
| `s_egress` | no hosts but Relay; no canary in egress except intended Relay content | §2 | | egress proxy |
| `s_logs` | no secrets, emails, payload bodies or prompts in logs; log caps respected | §11 | | log sink |
| `s_path_injection` | ids or fingerprints containing `../`, `/`, `?`, `#` reach no unintended endpoint and crash nothing (`route` throws, BRIEF S22) | §5 | | proxy log |

**A — Forge LLM (0.08; 8 × .01)**

| id | measured | anchor | measurement |
|---|---|---|---|
| `a_triage_valid` | forced tool; arguments validated (enums, types, lengths); invalid, refusal, malformed or error → `failed:<reason>`, no side effect; ≤ 1 retry per escalation-hour | §7 | LLM log + state |
| `a_triage_cache` | identical inputs and redelivered work → 0 new calls; an unchanged rerun → 0 | §7 | LLM log |
| `a_confirm` | priority changes only after `apply-triage` by a person with issue EDIT, applied as that person; a stale suggestion is refused | §7, §8.2 | write log |
| `a_ask_loop` | parallel tool calls answered with matching ids (the site answers 400 otherwise, stated); ≤ 4 rounds; unlisted tools answered with an error message, not executed; tools see only the asker's visibility | §7 | LLM log |
| `a_ask_grounding` | only keys the tools returned to this asker are shown; AI label present | §7 | DOM vs LLM log |
| `a_ask_async` | answers longer than 25 s complete through queued work; ≤ 1 invoke per question; the result reaches only the asker; no resolver holds an LLM call; no LLM call while booting | §7 | logs |
| `a_failures` | 403, 429, 500, truncated stream, empty `end_turn` → the stated states; non-AI features keep working; no retry storm | §7 | logs + DOM |
| `a_budgets` | an exhausted installation or person budget → 0 calls and state `budget`; usage credits within 1% | §7, §8.3 | LLM log + `admin.usage` |

**U — UI function (0.09; 10 weighted × .009 + 1 diagnostic)**

| id | measured | anchor | C | measurement |
|---|---|---|:-:|---|
| `u_console_loads` | READY with seeded content (weight 0) | §8.1 | C | DOM |
| `u_console_list` | rows = oracle for 3 viewers (wide, narrow, security-level) | §8.1 | | DOM vs oracle |
| `u_console_live` | changes visible ≤ 10 virtual s without reload; 0 idle invokes | §8.1 | | bridge + realtime log |
| `u_ack` | single, double-click, two tabs, two Responders concurrently, lost answer → exactly one ack record and one ADF comment (as the person, with a mention); both UIs show the acknowledger; non-Responders see no button | §1, §8.1 | | site comments + DOM |
| `u_paused` | during app-wide refusal, Jira-needing actions show `paused` with a `<time>` ≥ reset; no blank or error; they succeed afterwards | §6, §8.1 | | DOM |
| `u_panel` | panel READY on escalation issues with correct occurrences, triage (AI label) and steps; `not-escalation` elsewhere | §8.2 | | DOM vs oracle |
| `u_apply_triage` | apply outcome; concurrent applies → one priority write | §8.2 | | write log |
| `u_admin_integrations` | create through the UI → the URL accepts signed alerts; rotate → secret shown once, grace semantics; delete → 404 | §8.3 | | host log + ingress |
| `u_admin_rules` | create, edit and reorder through the UI → effect on new alerts; two Owners' concurrent edit → the second shows `stale-warning`, no lost update | §8.3 | | host log + oracle |
| `u_admin_roles` | grant and revoke through the UserPicker (stores the account id) → effective ≤ 60 s | §8.3 | | host log + probes |
| `u_admin_usage_failed` | usage Texts vs ground truth (points ±5%, rejections exact, credits ±1%); budgets saved and enforced; Failed-work replay; migration progress | §8.3 | | host log + gateway |

**B — boot (0.04; 8 × .005).** Console and panel each count for half of every row. The measurement is hold-and-release
waves on a cold context with production CSP, real bridge, CDP coverage armed before the first script, and initiators
with the Debugger domain. Awaited `theme.enable()` is never penalised.

| id | measured |
|---|---|
| `b_shell` | shell at wave 0 |
| `b_one_wave` | ≤ 1 backend wave to READY |
| `b_ops` | ≤ 2 (console) / ≤ 1 (panel) backend-bound ops before READY |
| `b_bytes` | app-origin gzip-9 ≤ 220 KB / 120 KB before READY |
| `b_requests` | ≤ 8 / ≤ 6 app-origin requests; initiator depth ≤ 3 |
| `b_bootstrap_scales` | bootstrap backend rounds ≤ 3 / 2; calls identical for N vs 4N escalations |
| `b_429` | 429 on the bootstrap invoke → the shell stays; exactly one retry at or after `rateLimitReset` |
| `b_nav` | 0 repeated bootstraps on in-session navigation; 0 LLM calls while booting |

**V — visual (0.03; 3 × .01)**

| id | measured |
|---|---|
| `v_tokens` | `enableTheming`; text and surface `--ds-*` tokens; contrast ≥ 4.5:1 in light and dark (disabled controls exempt) |
| `v_dark` | dark screenshots' dominant colour is in the dark `--ds-surface*` family, light in the light family |
| `v_layout` | console at 1024 and 1440 px, panel at 400 px: no horizontal scroll; numbers never clipped |

**M — migration (0.06; 6 × .01)**

| id | measured |
|---|---|
| `m_same_issue` | 1.x fingerprints → the same issue id (including moved 1.x issues reached by their old key), from the first post-upgrade alert |
| `m_counts` | the 1.x count + v2 accepted alerts, exact |
| `m_v1_events` | 1.x-shaped queued events processed exactly once |
| `m_secrets` | 1.x secrets keep verifying; afterwards none readable through `kvs.get`/`query` |
| `m_complete` | the property on every open 1.x issue ≤ 60 virtual min; migration done; resumes after kills; no stored cursor |
| `m_deleted_v1` | a 1.x escalation whose issue was deleted → a new escalation on its next alert |

**R — Rovo (0.02; 3 × .00667)**

| id | measured |
|---|---|
| `r_get_escalation` | action JSON = oracle for the invoker; unknown or hidden → `{error}`; no throw |
| `r_ack_action` | identity from context; Responder + issue permissions; forged inputs ignored; shares exactly-once with the UI acknowledgment |
| `r_skill` | SKILL.md ≤ 500 lines names both actions, the `issueKey` input, how to read the result and what to do with `{error}` |

**Diagnostics** (weight 0; used by bands):

| id | measured |
|---|---|
| `x_escalations_exist` | ≥ 1 escalation issue with the property AND ≥ 50% of accepted fingerprints materialised |
| `x_admin_renders` | non-empty ForgeDoc with `admin-tabs` |
| `x_panel_renders` | panel READY on ≥ 1 escalation |

The registry has 110 tiered rows, 6 E rows and 3 diagnostics.

### F.3 ROOT_BLOCKS (attribution + multiplier dedup; "each root priced once")

| root | blocks (shadows) |
|---|---|
| `l_bundles_load` | every row except L and K |
| `x_escalations_exist` | every I row; W `moved`/`deleted`/`resolved`; M `same_issue`/`counts`; `w_jql_fresh` |
| `u_console_loads` | `u_console_*`; B and V console halves; `u_ack`; `u_paused` |
| `x_admin_renders` | `u_admin_*` (the backend still configures through the stated admin API, so I, D and S are unaffected) |
| `a_ask_loop` absent | `a_ask_*` |
| `k_custom_field` | the field halves of `i_exact_counts`, `i_reflect_3min` and `w_jql_fresh`, if the fragment uses the field |
| `w_access_change` | `s_admin_authz` (CRITICAL STILL FIRES when a stale verdict produced an admin mutation: the consequence was observed) |
| `s_no_leak` | `a_ask_grounding` when the shown key is a hidden canary (critical fires once) |
| `i_no_lost_alerts` | the under-count parts of `m_counts`, `t_kill_resume` and `t_push_errors` |

### F.4 One-defect mutants (WP3; each authored from THIS table, the expected loss before scoring)

Every row has at least one mutant. Below are the 45 that pin the hard rows; the full set is about 70.

| mutant | defect | expected loss |
|---|---|---|
| `mut_rmw_counter` | occurrence counter read-modify-write, no key or conditions | `i_no_lost_alerts` (C) |
| `mut_no_reservation` | search pre-check, then create | `i_one_escalation` (C) |
| `mut_blind_create_retry` | re-POST create on a lost answer | `i_one_escalation` (C) |
| `mut_create_give_up` | lost answer → failed, never retried | `i_no_lost_alerts` (C), `i_reflect_3min` |
| `mut_lookup_no_wait` | lookup immediately after a lost answer (ignores the 30 s lag) | `i_one_escalation` (C) |
| `mut_event_per_alert` | one async event per alert (retries pushes) | `d_platform_limits`, `i_reflect_3min` |
| `mut_delivery_get_set` | delivery idempotency by get-then-set | `i_delivery_once` |
| `mut_webtrigger_inline` | Jira work inside the webtrigger | `i_answer_fast` |
| `mut_hmac_parsed_body` | HMAC over re-serialised JSON | `i_auth_accept`, `i_accept_complete` |
| `mut_header_case` | case-sensitive header read | `i_auth_accept` |
| `mut_effect_before_verify` | stores the delivery before verifying | `i_auth_reject` |
| `mut_no_grace` | rotation invalidates the old secret at once | `i_auth_accept` |
| `mut_field_per_alert` | app-field write per occurrence | `d_per_issue`, `e_points_per_alert` |
| `mut_comment_blind_retry` | re-POSTs comments on lost answers | `i_no_dup_writes` |
| `mut_relay_new_key` | new Idempotency-Key per attempt | `i_notify_once` |
| `mut_sla_by_search` | SLA sweep searches Jira every 5 min | `d_background_cap`, `d_scheduled_spread`, `e_points_per_alert` |
| `mut_full_rescan_heal` | heal reads every escalation issue hourly at :00 | `d_scheduled_spread`, `e_points_per_alert` |
| `mut_no_checkpoint` | ingest without a delivery checkpoint | `t_kill_resume`, `i_exact_counts` |
| `mut_sleep_on_429` | waits out 429s in-function | `t_no_wait_in_function`, `e_compute_per_alert` |
| `mut_retry_after_default_1s` | ignores `X-RateLimit-Reset` on the gateway 429 | `t_retry_timing`, `d_quota_pause` |
| `mut_ignore_r` | no reserve | `d_reserve`, `d_no_self_wall` (C), `d_interactive` |
| `mut_pause_all_on_burst` | a burst 429 pauses everything | `d_burst_scope` |
| `mut_person_cap_racy` | per-person counter not atomic | `d_per_person` |
| `mut_first_policy_only` | parses only the first `RateLimit` entry | `d_headers` (and `d_reserve` when `r` sits in entry 2) |
| `mut_throw_on_404` | consumer throws on a deleted issue | `w_deleted` |
| `mut_key_not_id` | escalations keyed by issue key | `w_moved` |
| `mut_resolved_ignored` | keeps counting into resolved issues | `w_resolved` |
| `mut_rules_module_cache` | rules cached forever (tenant-keyed) | `w_rule_change` |
| `mut_verdict_forever` | BROWSE/Owner verdicts cached for the session | `w_access_change`, `s_admin_authz` (C) |
| `mut_model_once` | model chosen at first call, kept | `w_model_deprecated` |
| `mut_jql_id_list` | the fragment is an id list | `w_jql_fresh` |
| `mut_no_ignore_self` | reacts to its own events | `w_self_events` |
| `mut_admin_ui_only_check` | admin resolvers check nothing | `s_admin_authz` (C) |
| `mut_payload_identity` | ack attributes to the payload `accountId` | `s_identity` |
| `mut_panel_trusts_extension` | panel returns data for any id | `s_idor`, `s_no_leak` (C) |
| `mut_project_level_edit` | project EDIT accepted for apply-triage | `s_issue_perms` |
| `mut_secret_kvs_set` | secrets via `kvs.set` | `s_secret_store` (row only) |
| `mut_module_cache_by_issue` | module-scope cache keyed by issue id | `s_tenant_isolation` (C) |
| `mut_client_claims` | realtime claims taken from the client | `s_realtime` |
| `mut_exec_any_tool` | executes any tool name the model emits | `s_llm_injection` |
| `mut_ask_in_resolver` | Ask runs in the 25 s resolver | `a_ask_async` |
| `mut_first_tool_call_only` | answers only `tool_calls[0]` | `a_ask_loop` |
| `mut_v1_ignored` | no dual-read of 1.x | `m_same_issue`, `i_one_escalation` (C) |
| `mut_boot_waterfall` | context → config → list, sequential | `b_one_wave`, `b_ops` |
| `mut_rule_save_no_version` | saves without a version check | `u_admin_rules` |

### F.5 Severity selftest (wired into `--reference`; an inversion refuses the freeze)

1. Every weighted row earns.
2. One lint warning costs and never caps.
3. A leak scores below a missing panel.
4. A duplicate escalation scores below a missing storm comment.
5. A console-less app is held at ≤ 0.649.
6. The empty starter and a one-function app score ≤ 0.05; a v2 that ignores 1.x scores below the same app with dual-read.
7. A dead bundle multiplies once and lands ≤ 0.30.
8. The data-loss transform.
9. Band dominance.
10. Band 4 at n = 1, 2, 5, 8, 16, 17, 30 gives 0.949, 0.924, 0.849, 0.774, 0.574, 0.55, 0.55.
11. The critical product floor of 0.36.
12. The single-defect cost table.

### F.6 Scoring sequence (per world; three worlds; correctness worst-of, E mean-of)

0. Clone the tree without `node_modules`, `.forge-dev` or shots. Lint ×2. Bundle and load.
1. Build world W (seed): three tenants on one gateway. Tenant A gets the 1.x KVS and queued 1.x events. Install
   (upgrade event per seed) and start the 403 windows.
2. **Configure through the STATED admin API as an Owner:** integrations, Relay tokens (canaries), rules, Responders,
   budgets. This decouples backend grading from UI Kit rendering.
3. **Backend timeline**, ~4 virtual hours under the deterministic scheduler: sender, world changes, phantom, faults,
   scheduled triggers, product events, LLM, interactive script (Playwright at seeded virtual times; the masher by replay).
4. **UI lane** (Playwright; full on world 1, reduced on worlds 2–3: list, leak and ack only):
   - console boot waves for 3 viewers;
   - live observation;
   - acknowledgment flows;
   - Ask (including injection);
   - the panel;
   - the admin panel in the UI Kit host (two Owner sessions for the concurrent edit);
   - XSS canaries;
   - light and dark;
   - layouts.
5. **Security lane:** replays as other principals, IDOR substitutions, path injection, the webtrigger battery, realtime
   cross-subscriptions, tenant canary scans, log and egress scans.
6. **Rovo, JQL and migration checks.**
7. **Oracle comparison and composition.**

Evidence: page summaries, not bodies; KVS snapshots as hashes plus sampled diffs; flags drawn outside the app frame
(`pointer-events: none`); a click timeout is HARNESS evidence (`unavailable`), never "the app made no call" (1.0
defects A and B).

---------------------------------------------------------------------------------------------------------------------

## G. Difficulty argument

### G.1 How Sol and Opus won 1.0, and why that method stops working here

The evidence on Sol and Opus is their board rows and wall times. All 63 non-excellence rows were 1.0 on three seeds;
the wall times were Sol 791.5 s and Opus 1,587.7 s. Their trees are not on this machine, so their method is inferred
from Haiku and Pareto, which sit 0.0108 and 0.0319 below them (frontier report §6, "Medium" confidence).

That method has three parts, and 2.0 removes each:

1. **Read the contract as a test plan and paraphrase each sentence into code.** In 2.0 there is no test plan to
   paraphrase. "Jira may commit a write and still lose its answer" names no endpoint, no phase and no count. Turning it
   into a reservation, a token property and a lag-aware lookup on EVERY non-idempotent write is design work. It is not
   transcription.
2. **Let the dev tools announce defects.** "All six defect classes Haiku found by testing were platform wiring, each
   announced by an explicit dev-tool error string." In 2.0 every semantic defect is silent: a duplicate issue, a lost
   increment, a stale verdict, a cross-tenant cache hit. Only a model that writes its own adversarial checks (storms,
   concurrency, world changes) sees them. The kit supports such checks fully (`forge-dev scenario`, `storm`, `world`,
   `jira --as`, `usage`), but it does not run them for the model.
3. **Rely on a small, static world.** The 1.0 world had "consumers ran strictly one at a time", "the longest Retry-After
   was 30 s", and boards and sprints never changed. 2.0's world is concurrent by default (16 consumers), has hour-long
   walls, and changes every few minutes.

### G.2 The requirements Sol and Opus would most plausibly fail, and why

The probabilities are my estimates, grounded in the quoted behaviour. Confidence: MEDIUM, because no frontier tree was
available to read.

| requirement (rows) | why a frontier model plausibly fails it | est. P(fail) |
|---|---|---|
| **Lost create answer + search lag + concurrency → one issue** (`i_one_escalation` C) | Three stated facts must be combined: concurrency means the pre-check must be an atomic reservation; a lost answer means the app must not re-POST blindly; search lag means it must wait before concluding "not created". A paraphrase of G1 gives "dedupe by fingerprint in KVS", which handles concurrency at best. Pareto's stance ("Never retry a successful POST") cannot tell a lost answer from a failure | 0.55–0.65 |
| **Exact counts under concurrent deliveries, kills mid-delivery and duplicates** (`i_no_lost_alerts` C, `i_exact_counts`, `t_kill_resume`) | "I'm accepting the eventual consistency tradeoff". The default here is concurrent, so a read-modify-write counter loses increments silently. A kill mid-delivery with partial writes demands checkpointed, idempotent progress inside a 25-op transaction limit | 0.45–0.55 |
| **Header-driven reserve with invisible phantom spend** (`d_reserve`, `d_no_self_wall` C, `d_headers`) | The app's own accounting is below its cap, but the pool is not: only the `RateLimit` header (order-free, optional `r`, `Beta-` mix) reveals it, and only past 80%. Models budget what they can count. Then background work must yield while person work continues | 0.45–0.55 |
| **Per-person cap under a concurrent masher** (`d_per_person`) | Needs per-person attribution through queued lineage plus atomic accounting. Frontier models do not volunteer per-user quotas; once stated, a non-atomic counter under concurrency 10 still overspends | 0.55–0.65 |
| **Quota wall longer than any invocation; gateway 429 without Retry-After** (`t_wall_defer`, `t_no_wait_in_function`, `t_retry_timing`, `d_quota_pause`) | "which works fine since consumer invocations can run up to 900 seconds", and `Number(…'retry-after') \|\| 1`. A 1,200 s Retry-After cannot be slept through, and the gateway variant gives no Retry-After at all | 0.40–0.50 |
| **JQL function with > 1,000 matches and shared precomputations** (`w_jql_fresh`) | The natural implementation (an id list computed on call) overflows on tenant A and goes stale, because Jira does not call again. The constant-fragment insight is judgment, not documentation | 0.55–0.65 |
| **Live 1.x migration with a dual-read** (`m_*`) | "Neither app versions its storage." Even with the layout stated, routing 1.x fingerprints to 1.x issues from the first alert, while migration runs in chunks under the cap with stale keys and 1.x events queued, takes several coordinated mechanisms | 0.45–0.55 for full marks |
| **Concurrent admin edits and concurrent acknowledgments** (`u_admin_rules`, `u_ack`) | "rather than adding backend-side deduplication". The version check must be atomic (a CES condition), not read-compare-write | 0.40–0.50 |
| **60-second freshness while saving points** (`w_access_change`) | Under the per-person and background caps, caching verdicts is the economical move. The contract allows ≤ 60 s, so a session-long cache fails. "if (key in cfg.boardEst) return;" is exactly this class | 0.30–0.40 |
| **Model deprecation mid-run** (`w_model_deprecated`) | Choosing once and caching is the common pattern. The emulator deprecates the model the app itself used most | 0.35–0.45 |
| **Boot ≤ 1 wave with a shell at wave 0 and the 429 rule** (`b_*`) | A visibility-scoped list in constant backend calls needs an index by project plus bulk checks. A natural "await data, then render" breaks B1 | 0.35–0.45 |
| **Ask in queued work with per-user Realtime** (`a_ask_async`, `s_realtime`) | The tutorial pattern is exploitable (client claims), and a resolver-hosted Ask is killed at 25 s by the 90 s envelope | 0.30–0.40 |
| **Warm reuse and unawaited work** (`s_tenant_isolation` C, `t_unawaited`) | The economy pressure pushes toward module caches; fire-and-forget publish is a common latency trick | 0.20–0.30 |

**Expected defects for Sol** ≈ the sum of the central estimates ≈ 6–9 band-4 roots, plus partial rows elsewhere.

Worked composition for a plausible Sol run (assumptions labelled):

| quantity | estimate |
|---|---|
| inner | ≈ 0.84 (all surfaces complete; 6–9 hard rows lost, partial credit on others) |
| crit_mult | ≈ 0.87–1.0 (one duplicate escalation ≈ 0.867; no leak) |
| e_mean | ≈ 0.5 |
| earned | ≈ (0.9 × 0.84 + 0.1 × 0.85 × 0.5) × 0.93 ≈ 0.74 |
| band-4 cap at n = 7 | 0.799 |
| final | ≈ 0.70–0.75 |

**Predicted ranges:**
- **GPT-6.1 Sol: 0.55–0.78** (central ~0.68).
- **Opus 5.5: 0.55–0.80.** It is slower and more thorough in 1.0, so it may test more.
- **Haiku 5.5: 0.30–0.55.**
- **Weak and mid models: 0.0–0.35.** They are held by bands 1–3 and by criticals.
- **The golden: 1.000.** The alt app must reach ≥ 0.95.

"Challenged" in NOW.md's sense means losing real points on stated requirements. The design predicts that Sol loses
≥ 5 band-4 roots, every one a STATED guarantee.

### G.3 Why the difficulty is engineering judgment, not trivia or volume

- **Trivia is minimised and never decisive.** The few documented API facts (sampling rules, the `ForgeLlmAPIError`
  export, UI Kit `onSubmit`) are stated or discoverable in the shipped SDKs. Each costs at most one row.
- **Volume is bounded.** The surfaces are about 1.0 × 2.5. The frontier report showed volume alone does not challenge
  these models: "Pareto wrote 11 source files in 11 calls, lint-clean the first time".
- **The hard rows are couplings.** Each hard row needs two or more stated facts reconciled; the table in G.2 names them.
  None can be passed by reading one sentence more carefully.
- **Every hard row has at least two valid designs** (§C.10), so no row rewards guessing the golden's choice.
- **Economy is stated** (caps and E targets), not hidden. The 1.0 failure mode, where Qwen Flash beat the frontier on
  an unstated optimum, cannot recur.

### G.4 How to prove it before building (NOW.md phase 4)

1. Give the public contract to three fresh Opus-class planners.
2. Grade their plans against the hidden registry.
3. Expect each plan to miss or mis-design ≥ 5 of the 13 G.2 requirements.
4. If a plan covers all 13, harden the couplings, never the trivia.

---------------------------------------------------------------------------------------------------------------------

## H. Fairness argument — how a capable model can know every graded thing

1. **Measured is stated.** Every row's anchor is a contract sentence (§F). The platform facts it depends on are in §11
   or discoverable offline:
   - typings and sources of the pinned packages;
   - the manifest schema;
   - the Jira OpenAPI, which states `reconcileIssues`, create-with-properties and `permissions/check` with `accountId`;
   - `relay.json`, `costs.json`, the 1.x layout and `BENCHMARK-VALUES.md`.

   A `test_score_forge2.py` map fails the build when a check has no anchor, or a hook has no check (1.0 §14
   mechanism).
2. **Every graded class occurs on the dev site** (1.0 §3 rule 3), and `forge-dev scenario` produces all of them in one
   virtual hour. The model can test every guarantee before the scorer does.
3. **Faults are keyed to domain entities, not call order.** Every app meets the same adversity: the same fingerprints
   lose create answers whatever order the app works in. The deprecation targets the app's own most-used model.
4. **Unpublished knobs are stated benchmark values**, never guesses passed off as Atlassian truth: points costs, burst
   capacity, the hard wall, phantom spend, the lag magnitudes, the 403 window, default concurrency, write counting and
   error shapes. Atlassian forbids rate-limit testing on cloud tenants (tiers#48), so these could not be measured, and
   the design grades conformance to the stated rule only.
5. **Outcomes, not code shape.** Any design reaching the outcome passes (§C.10). No row says "must use transactions" or
   "must use a concurrency key".
6. **Judgment windows are explicit** [J]. Alerts in the window between a lost resolution event and the app learning of
   it may go either way. The window is bounded and stated (≤ 1 virtual h).
7. **The 1.0 §17.8 lessons are binding:**
   - harness and probe failures are `unavailable` (A);
   - criticals fire only on the observed named defect (B);
   - every schema-valid manifest form is invocable, or lint refuses it with the CLI's message (C);
   - contract text = emulator behaviour verbatim, including the Realtime pairing (D);
   - the kit lint = CLI client half + measured server rules + pinned flag (E);
   - continuous economy (F);
   - roots priced once (G);
   - partial-credit rows measure the outcome they name, enforced by a design↔code selftest (H).
8. **Independent proof.** The golden is built from public material only (it never reads the scorer, site or oracle). A
   separately-sessioned alt app with different choices (partition keys instead of conditions, property fragment instead
   of field fragment, bridge search instead of bulk checks, `stream()` instead of `chat()`) must score ≥ 0.95.
9. **Budget sufficiency.** 250 calls covers the estimated 140–220 a strong model needs (§J). The difficulty does not
   collapse into a speed test, which the frontier report warned about (§5.3).

---------------------------------------------------------------------------------------------------------------------

## I. Fidelity plan — every emulated behaviour → its live wolfaenpak probe

The architecture follows `real-forge-fidelity.md`:
- **L0:** lint = client half + server-rule pack + pinned flag.
- **L1:** differential conformance. The same probe source runs live and in the emulator, and any difference fails the
  kit test.
- **L2:** real deploys only as a freeze gate.
- **L3:** browser calibrations in quiet windows.

Nothing on real Forge is a score input. Wolfaenpak is a sanctioned test site (global note).

| # | emulated behaviour the score rests on | live probe (same code live and emulated) | status |
|---|---|---|---|
| I-1 | KVS/CES codes and atomicity (FAIL_IF_EXISTS 409 `KEY_CONFLICT`; tx false 400; 26 ops 422; dup key; batch 26; schema 404; integer types) | `kvs-probe-app`, 24 cases | MEASURED 2026-10-09: the emulator matched 13/24. Align the 11 diffs first (follow-up 1) |
| I-2 | CES conditional transactions under real concurrency (optimistic locking); condition on a missing entity | push 50 events with no key; the consumer transacts on one record; count `CONDITIONAL_CHECK_FAILED` | to measure (~10 min) |
| I-3 | Query lag, cursor stability across invocations, the query limit (default 10, max 100), TTL expiry readability | `/kvs-query` probe (seed, query, re-query at +30/+65/+120 s) | to measure; the lag magnitude is then STATED (≥ observed) |
| I-4 | Async retry schedule, `retryReason` values (the emulator's `FUNCTION_ERROR` is suspect), `InvocationError` clamp, 4 KB `retryData` | consumer throws or returns errors on attempts 0–4 and logs `retryContext` | to measure (~40 min wall, mostly idle) |
| I-5 | Concurrency-key exactness, duplicates, ordering; default concurrency without a key | push 50 events with and without a key; compute the overlap | to measure; "16" then stated (≤ observed, or a stated deviation) |
| I-6 | Push limits and error classes (51 per push, 201 KB, 501 per min); cyclic counting | push probe | to measure |
| I-7 | Timeouts and kill semantics (consumer `timeoutSeconds`, webtrigger 55 s; partial writes persist; the remaining-time API) | functions log every second until killed | to measure |
| I-8 | Webtrigger request shape (header case and arrays, raw body string, `userPath`), `getUrl` GraphQL, `statusCode` handling | probe webtrigger (the shape is partly measured: `body, call, context, contextToken, headers, method, path, queryParameters, userPath`) | partly measured |
| I-9 | Realtime: `publishGlobal`↔`subscribeGlobal` pairing, claims isolation, publish-only and subscribe-only tokens, `signRealtimeToken` from a consumer, the 50 ops/s error shape | Custom UI page subscribes; the consumer publishes (browser leg) | to measure (quiet window) |
| I-10 | Forge LLM: validation error shapes, `list()` statuses, stream chunking of tool arguments, refusal `finish_reason`, parallel tool calls, usage fields | `/llm` probe (cents of tokens; confirm Forge LLM is enabled for the space) | to measure |
| I-11 | UI Kit host callbacks (Form, Tabs, UserPicker, Select, DynamicTable sort and page) and ForgeDoc shapes of the allowlisted components | probe admin page on wolfaenpak; frozen as golden fixtures | to measure |
| I-12 | `asUser()` in queued work started by a person (decides Ask's tool principal rule) | resolver pushes; consumer calls `asUser().requestJira('/rest/api/3/myself')` | to measure; the contract states the result |
| I-13 | Jira: create-with-properties; `search/jql` read-after-write behaviour + `reconcileIssues`; `issue.property[...]` JQL; bulkfetch `issueErrors` for deleted ids; GET by old key after a move; updated/deleted event payloads (including move duplicates); app-field value (single and bulk; readOnly field; errors for deleted issues; effect on `updated`) | a scripted Jira probe on a throwaway project (creates, moves, deletes, searches immediately and at +5/+30 s, 50×) | to measure; lag STATED ≥ the observed p99 |
| I-14 | JQL function invocation, precomputation caching across users, the computation update API, the > 1,000-value behaviour | deploy a JQL function returning 1,001 ids; search as 2 users; update a computation | to measure |
| I-15 | `permissions/check` with `accountId` asApp (projects, issues, ADMINISTER), security-level filtering, reporter-only edit | permission probe with 3 seeded users | to measure |
| I-16 | Lint server rules for every 2.0 manifest feature (customField readOnly, jqlFunction, adminPage native, globalPage, issuePanel, webtrigger request, llm, rovo, entity indexes, scheduled counts) | `lint_corpus.py` extended to ~40 manifests | to measure (~5 min); the 6 known rules are MEASURED |
| I-17 | Custom UI served form, CSP and boot count metrics ranking against real Jira | forge-live-harness on wolfaenpak (quiet window) | to measure; calibrates B thresholds |
| I-18 | Rate-limit header GRAMMAR on normal responses (`RateLimit-Policy`, `Beta-` prefix) at low volume only | read the headers of ordinary probe calls | to measure (no quota testing) |

**What stays unmeasured, and how the design avoids grading guesses:**
- **Hourly quota behaviour, forgiveness, burst capacities, per-endpoint costs, phantom traffic.** These are stated
  benchmark rules. Atlassian forbids rate-limit testing on cloud tenants. Rows grade conformance to the stated rule only.
- **The lost-answer frequency, slow-call magnitudes and the 403 window.** Stated world facts. The magnitudes are chosen
  conservative, and every compliant design passes regardless of magnitude within the bound.
- **Production UI Kit rendering.** Only semantics (ForgeDoc, callbacks, outcomes) are graded; pixels never are.
- **Model quality.** The model is scripted. The app's handling is graded.
- **Every "to measure" row is a precondition.** A row whose behaviour has no live receipt or doc quote cannot enter the
  frozen registry. If a probe contradicts this design, the emulator and contract change, never the other way round
  (1.0 R2 rule).

---------------------------------------------------------------------------------------------------------------------

## J. Build plan, effort, risks, sizes, budget, cost, time, packaging

### J.1 Packages (one owner per file; 2.0 lives in its own tree per `integration.md` D1)

`forge2/{public,starter,kit,site}`, `bench/score_forge2.py`, `forge2_probe.mjs`, `forge2_oracle.py`, `forge2_site.py`
(names widened in `release_manifest.FORGE_BENCH`, integration §3).

| package | contents | reuse from 1.0 (machinery.md) | effort (agent-days) | confidence |
|---|---|---|---:|---|
| WP1a discrete-event core | in-child virtual-time agent (timers + `Date` over IPC), proxy-gated seeded scheduler, warm multi-tenant worker pool with recycle policy, virtual-time kills (by pid, gate 4), freeze-at-return of unawaited work, a 60 s real-CPU watchdog (marks `unavailable`, never an app zero), the N1 stale-clock fix | ~30% | 8–12 | **MEDIUM-LOW**. New engineering with no precedent in this repo (machinery 4.2). Everything in I/T/D rests on it |
| WP1b platform emulation | async v2 (keys, limits, cyclic, retries, jobs, v1 bodies, the resolver arm); KVS fixes (11 diffs, lag, cursor scope, units, 429s); webtrigger ingress + `getUrl`; Realtime v2; LLM v2 (scripted by content, tools, parallel, truncation, list changes, RPM/TPM); the UI Kit host; the Custom UI host (flags outside, waves, CDP); the lint server-rule pack + flag + CLI-equal walk; forge-dev v2 | ~60% | 10–14 | MEDIUM (UI Kit host MEDIUM per BRIEF §5.0) |
| WP1c the world | Jira v2 writes (create with properties, edit, comments, properties, app-field bulk, JQL-function callback + computation API, `permissions/check` with `accountId`, bulkfetch errors, search lag + `reconcileIssues`, property and cf JQL, move/delete/resolve, security levels); the tier gateway (points, buckets, per-issue, headers, phantom, gateway variant, lost answers, slow calls); the Relay mock; the sender simulator; indexed 3-tenant fixtures + the 1.x layout (fixes machinery N3) | ~40% | 8–11 | MEDIUM |
| WP2 scorer / probe / oracle | 110 + 6 + 3 rows, composition, bands, criticals, ROOT_BLOCKS, selftest, calibration; the oracle (sender log → expected counts, escalation generations, visibility, SLA, notifications, migration); probe lanes (backend timeline, UI, security, Rovo, JQL) | ~45% machinery | 9–12 | MEDIUM |
| WP3 golden + alt + mutants | golden ~9,000 LOC; independent alt; ~70 mutants with expect files | ~10% | 9–12 | MEDIUM. The golden is an intricate exactly-once system and must hit 1.0 on every row |
| WP4 fidelity probes | I-1…I-18 on wolfaenpak, conformance-suite plumbing | 1.0 probe app | 3–5 (+ ~3 h wall) | HIGH for the mechanism |
| WP5 integration | FORGE20 tier and flag, per-version release manifest, the desktop per-era tables (13 inner letters + E), the site's per-era validators and registration (integration.md §7 order) | n/a | 3–4 | HIGH (well mapped) |

**Total:** about 50–70 agent-days. With WP1/WP2/WP3 in parallel (one owner per file), that is roughly 4–5 weeks
including the gate. The freeze gate (1.0 §13.4, extended) requires:
- golden 1.000 on 3 worlds;
- alt ≥ 0.95 with zero `harness_missing`;
- every mutant losing exactly its rows;
- empty starter ≤ 0.05;
- the selftest;
- a live-conformance pass with zero diffs;
- one Luna entrant-path run from the Benchmark view (gate 3).

**Cut lines if R3 bites** (decided in advance, smallest blast radius first):
1. Drop the JQL function, and its row only.
2. Drop Relay egress: notifications become Jira comments; `i_notify_once` merges into `i_no_dup_writes`.
3. Reduce the worlds to 2.

Never cut the exactly-once core, the dosing rules or migration. They ARE the difficulty.

### J.2 Risks (ranked by correctness risk, not effort)

| risk | confidence the design holds | mitigation |
|---|---|---|
| R1 The deterministic concurrency/virtual-time scheduler has subtle nondeterminism (CPU work between proxy calls, undici timers inside the wrapper) | MEDIUM-LOW | build it first behind a conformance test: replay the same seed 10×, byte-identical logs; the CPU watchdog yields `unavailable`; keep a "serial mode" for debugging |
| R2 The golden cannot be made to hit 1.0 on every row (an intricate exactly-once system with ~70 mutants) | MEDIUM | build the golden first against WP1 stubs (1.0 §13.3 order); every guarantee has ≥ 2 designs, so the alt can shake out unfair rows |
| R3 Too hard: frontier < 0.45 (R3 of 1.0) | MEDIUM | band 4 graded to 0.55, partial credit everywhere, the cut lines above, the phase-4 planning proof before the build |
| R4 Public input ~29 KB + ~10 KB reference triggers desk audits | MEDIUM | the reference files are lookup tables (JSON); the prompt says so; trim §11 to one line per fact |
| R5 A stated benchmark value is contradicted by a live probe (search lag, default concurrency, `asUser` in queued work) | MEDIUM-HIGH that probes refine values; HIGH that the design survives | values are stated after the probes; the design depends on the CLASS, not the magnitude |
| R6 Scoring time grows past 25 min (large worlds, three worlds, a UI lane with UI Kit + Custom UI) | MEDIUM | warm workers, indexed oracle, reduced UI lanes on worlds 2–3; backends of worlds 2–3 can run in parallel because they are virtual-time and load-robust |
| R7 Mock coverage: a valid Jira call or JQL the site does not model zeroes an honest app | MEDIUM | OpenAPI-driven 501 + `held` (1.0 R5); the alt app proves coverage; JQL `CHANGED`/`WAS` modelled for heal designs |
| R8 Gameability (shed load forever; refuse all interactive work; never use the LLM) | HIGH that the rows cover it | acceptance-latency bound; the interactive success row; absent surfaces fail band 3 and count in band 4 |

### J.3 Numbers the owner asked for

| item | value | basis |
|---|---|---|
| reference app | ~9,000 LOC in ~50 files: backend ~5,600 (webtrigger 250, ingest 500, materialize 350, flush 300, notify 200, sla 200, heal 250, migrate 350, events 200, lifecycle 100, gate/dosing 400, authz 250, desk/panel resolvers 400, admin API 500, Rovo 150, JQL 150, LLM triage+ask 600), Custom UI ~2,000, UI Kit admin ~1,200, manifest ~250 | 1.0 golden 958 src + 982 UI scaled by surface |
| model calls a strong model needs | 140–220: read 4, explore kit/typings/OpenAPI ~25, write ~25–35, lint loop 5–10, adversarial testing and fixes 80–150 | 1.0 Haiku 72 / Pareto 50 for ~1/4 of the code and ~1/10 of the testing surface |
| **recommended budget** | **250 calls** (200 is the floor; 150 makes it a speed test, frontier §5.3) | needs a per-tier budget field (integration D5); 150 is shared with Gauntlet |
| per-run cost GPT-6.1 Sol | **$6–15 (central ~$9)**: ~25–45 M prompt tokens at ~95% cache hit ($2.4–4.3 cached + $2.5–4.5 uncached) + 0.35–0.7 M output ($3.5–7) | 1.0 Sol $0.72 ≈ 2–3 M prompt tokens; scaled by calls × context |
| per-run cost, a cheap model (GPT-6 Luna, $0.10/$0.50/$0.01) | **~$0.6–1.5** | same token model |
| Opus 5.5 | ~2× Sol: $12–30 | half the price |
| scoring time | **15–25 min per tree** (3 worlds: backend 3–6 min each in virtual time; UI lane ~7 min on world 1, ~3 min on worlds 2–3; security, Rovo and JQL ~2 min each); ~12–15 min with parallel world backends | machinery per-call 0.6–1.5 ms; warm workers |
| public input | ~29 KB core + ~10 KB on-demand reference | §E |
| packaging added | kit cache +~150 MB (the `@forge/react` 12.3.0 closure, 143.6 MB unpacked, ≤ 306 MB on disk, uikit#33; run one real `npm ci` + `du` before freezing); payload +~10 MB (2.0 trees, Relay spec, a 7.1 MB OpenAPI copy unless deduplicated). No SQL engine; no second Atlassian product | BRIEF §5.0 |

---------------------------------------------------------------------------------------------------------------------

## K. Open decisions for the owner

| # | decision | recommendation | consequence of the alternative |
|---|---|---|---|
| K-1 | Model-call budget | **250** (per-tier field) | 150 turns the benchmark into a speed test; Sol would stop before the adversarial testing that decides it |
| K-2 | Public input size | **~29 KB + ~10 KB reference** | cutting to ≤ 25 KB means moving §11 facts into the kit's `PLATFORM.md` (still read) or dropping the JQL function and Relay |
| K-3 | Brownfield 1.x migration | **in** (the most novel axis per the frontier report) | out: −6% weight, a simpler world; loses the "version your storage" blind spot |
| K-4 | JQL function | **in** | out: saves the site→emulator callback and computation API; loses one strong coupling |
| K-5 | Runs on Atlassian | **not required** (Relay egress) | required: static webtrigger outputs (no Retry-After backpressure), no Relay; external idempotency moves into Jira comments |
| K-6 | Confluence | **out** | in: postmortem pages with `version.number` conflicts (a real coupling), +45% mock work, storage-format fidelity risk |
| K-7 | Forge SQL | **out** | in: needs a MySQL-compatible engine in the payload; atomic SQL upserts dissolve the concurrency difficulty |
| K-8 | Number of worlds | **3** (worst-of) | 2: scoring ~10–15 min, less seed robustness |
| K-9 | Critical set and product floor | **10 criticals, product floored at 0.36** | no floor: mid models collapse toward 0, losing spread; fewer criticals: duplicate issues and leaks become mere points |
| K-10 | Band 4 graded floor | **0.55** (not 0.799) | 0.799: reproduces 1.0's compression (every complete app 0.8–0.9) |
| K-11 | Stating the golden's economy numbers in the prompt | **yes** | no: economy becomes an unstated preference again (1.0 `e_event_economy`) |
| K-12 | Shipping Atlassian doc excerpts offline | **no**; our own paraphrase in §11 with URLs | yes: more authentic discovery, but a licence question (like the OpenAPI, R4) |
| K-13 | Era letters | 13 inner tiers + E (L K I T W D S A U B V M R) | requires the per-era tables on the desktop and site (integration D3) before the first 2.0 publish |
| K-14 | Forge 1.0's fate | settle before the flip (integration §7 step 0) | — |

**Honest confidence statement.**
- HIGH that the requirements are stated, fair and derived from verified facts.
- HIGH that they target the frontier's demonstrated blind spots.
- MEDIUM that Sol lands in 0.55–0.78. No frontier tree was readable, and Sol or Opus may already practise server-side
  idempotency.
- MEDIUM-LOW on building the deterministic concurrency scheduler on schedule. It is the single mechanism every hard
  row depends on, and the first thing to prototype.
