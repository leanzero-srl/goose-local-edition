# Forge 2.0 — the lean build spec ("Scope Ledger 2", a brownfield upgrade)

The single source of truth for tonight's build (2026-10-09/10). Every package uses THESE numbers, names and interfaces.
`DESIGN-DRAFT.md` and `panel/` are reference only; they are NOT the build plan (owner: "find the more effective way",
and the global NEVER OVER-ENGINEER rule). Facts come from `research/BRIEF.md` (verified) — never from memory.

## 0. The idea

Reuse Forge 1.0 (kit ~85-90 %, site ~70 %, scorer ~50 %, probe, oracle, golden). The entrant inherits **Scope Ledger v1**
(the 1.0 golden app, now `forge2/starter/`) installed on a much bigger Jira site with live v1 data, and ships **v2**.
Difficulty comes from operating under production conditions (scale against a points quota, invocation time limits, a
world that changes mid-run, a live data migration, security and a UI Kit admin panel) — stated as GUARANTEES, never as a
list of the faults the grader injects. The 1.0 contract's product rules (FORGE-CONTRACT.md §1-§8) stay, adapted.

## 1. What v2 must do (each item is a graded, stated requirement)

| # | requirement (contract states it as a guarantee) |
|---|---|
| R1 | **Live migration.** v1 rows live in entity `scope-change` (v1 schema, see starter manifest). v2 stores the ledger in a NEW entity `scope-ledger` (§2.4). Every v1 row appears in `scope-ledger` exactly once with its original changeId and time; v1's entity is left intact; migration runs while events keep flowing, survives being cut off by time limits (resumable), and completes within the first 2 virtual hours after the upgrade. Progress is visible in the admin panel. |
| R2 | **Tier 1 dosing at scale.** The site is ~1,000 issues. Jira charges points (RATE-MODEL.json, §2.1). The app must finish its backfill/reconcile correctly while background work never takes more than 70 % of any hour's quota, never receives a quota 429 for a person-facing request, honours Retry-After, and reacts to `RateLimit-Reason` (quota → pause all background until reset; burst → slow that endpoint; per-issue write → delay that issue). |
| R3 | **Invocation limits in virtual time** (§2.2). Work that cannot finish in one invocation continues in the next (queue continuation, next scheduled run). A Retry-After longer than the time left must not be waited out in-function (return `InvocationError` with `retryAfter` from a consumer; stop and resume later from a scheduled run). A killed invocation must leave no partial duplicate. |
| R4 | **A world that changes mid-run.** Sprints close; issues move to another board's sprint; a board's estimation field changes; issues are deleted; a person loses browse permission on a project. Guarantees: §2.5. The dev site exercises every class at least once; the scoring schedule is private. |
| R5 | **Admin panel in UI Kit** (`jira:adminPage`, `render: native`, `@forge/react`). Controls and labels in §2.6. Every admin action is authorized SERVER-SIDE (any user who can load a surface can invoke its resolvers): the resolver checks the caller holds Jira's global `ADMINISTER` permission via `GET /rest/api/3/mypermissions?permissions=ADMINISTER` asUser. Identity always from `req.context`, never from the payload. |
| R6 | **CI deployment web trigger** (static `webtrigger`). Signed events (§2.7). Invalid/missing signature, tampered body or stale timestamp → `401`, zero side effects; replayed eventId → `200`, no second effect; valid → `202` and the referenced issues' ledger rows/issue show "Deployed to <env>". Secret from the admin panel, stored with `kvs.setSecret`, compared with `crypto.timingSafeEqual`, never returned by any resolver (masked `••••<last4>` only). |
| R7 | **Custom field** `jira:customField` key `scope-status`, type `string`, read-only, written by the app via `POST /rest/api/3/app/field/value` (asApp, bulk: `{updates:[{customField, issueIds, value}]}`) or `PUT /rest/api/3/app/field/{fieldIdOrKey}/value` (`{updates:[{issueIds, value}]}`) — Jira's app field-value API as the shipped OpenAPI (`forge2/kit/openapi/jira.json`) has it; there is no bulk PUT. Value per issue in an ACTIVE sprint: `committed`, `added +<points>`, or `removed`; issues in no active sprint: empty. Fresh within the same virtual hour as the change. |
| R8 | **Forge LLM, properly.** v1's explanation stays (numbers only from data, hidden data never in a prompt). New: any tool call the model returns is validated against the viewer's sprint scope before acting (a model may be manipulated by issue text); a 429 with no Retry-After backs off (≤ 3 attempts per minute); a response with no `finish_reason` is a failure, never shown; identical explanation requests within 10 virtual minutes are served from cache (no new LLM call); the admin kill switch and daily token budget are enforced server-side. |
| R9 | **Custom UI boot budget** (count-based): the widget view and the sprint modal each make ≤ 1 `invoke` before their first data paint, load ≤ 150 KB of JS+CSS before it, and request no external origin; the admin page makes ≤ 1 invoke before its first render. |
| R10 | **v1 keeps working** under all of the above (the 1.0 checks, fixed per forge/DESIGN.md §17.8, become regression rows). |

## 2. The numbers and shapes (contract + RATE-MODEL.json carry these verbatim)

### 2.1 Rate model (RATE-MODEL.json, public; the benchmark's own world — Atlassian publishes no costs)
- Quota: **2,400 points per installation per virtual hour**, reset at the top of each virtual hour (hard wall). This is
  the app's fair share of its 65,000-point Tier 1 pool across ~27 tenants (BRIEF §6: Tier 1 = 65,000/h per app, shared).
- Background (triggers, consumers, scheduled, web trigger) may use at most **70 %** of the hour (1,680 points);
  person-facing requests (resolvers invoked from a UI surface or Rovo) must never be refused for quota.
- Costs: GET 1 · `GET/POST /rest/api/3/search/jql` 1 + 1 per 50 issues returned · `POST /rest/api/3/changelog/bulkfetch`
  2 per call (≤ 1,000 issues) · any other POST/PUT/DELETE 2 · `POST /rest/api/3/app/field/value` and `PUT /rest/api/3/app/field/{fieldIdOrKey}/value`
  1 + 1 per 50 updates (≤ 200 updates per request; a started block of 50 counts) · agile GETs 1.
- Burst: per endpoint (method + path template) token bucket, capacity 30 points, refill 5 points/s. Per-issue writes:
  at most 1 write per issue per 2 s.
- 429 body `{"errorMessages":["Rate limit exceeded"]}`, headers `Retry-After: <s>` and `RateLimit-Reason:` one of
  `jira-quota-tenant-based` (Retry-After = seconds to the next hour), `jira-burst-based`, `jira-per-issue-on-write`.
  Every response carries `X-RateLimit-Limit: 2400`; when < 20 % of the hour remains, also `X-RateLimit-Remaining` and
  `X-RateLimit-NearLimit: true`.

### 2.2 Virtual time and invocation limits (BRIEF §3 values)
- Each proxied request advances its invocation's virtual clock by: GET 120 ms, search page 300 ms, bulkfetch 600 ms,
  writes 200 ms. In-function waits (`setTimeout`/sleep) count at face value in virtual time.
- Limits: UI resolver **25 s**; consumer and scheduled trigger **55 s** default, up to **900 s** with `timeoutSeconds`;
  web trigger and Rovo action **55 s**. Exceeding the limit kills the invocation (no result).
- Async: a consumer that returns `InvocationError` (retryAfter ≤ 900 s) or is killed is redelivered (at least once,
  any order). Scheduled triggers run hourly and are not retried (a failed run waits for the next). Product-event
  triggers: up to 4 retries. Consumers run concurrently (§2.8 S1).

### 2.3 Scale (scoring sites; the dev site is the same shape, another seed)
3 projects, 4 scrum boards (2 estimate with field A, 2 with field B — ids vary per seed), 6 active sprints, 2 future,
6 closed, **~1,000 issues** (~300 in active sprints), 6 virtual hours scored after the upgrade, ~200 relevant changes
plus ~800 irrelevant issue updates, v1 rows preloaded for the first 2 days of each active sprint.

### 2.4 Storage
- v1 (unchanged, preloaded): entity `scope-change` exactly as in `forge2/starter/manifest.yml`.
- v2: entity `scope-ledger`, attributes `sprintId` string, `at` float, `changeId` string, `kind` string, `issueId`
  string, `issueKey` string, `estimate` float, `boardId` string, `estimateField` string, `deleted` boolean,
  `deployedEnvs` string; index `by-sprint` partition `[sprintId]` range `[at]` (exactly ONE range attribute — real Forge
  refuses two at lint and deploy). Plus any KV keys the app wants (migration cursor etc.).
- KVS errors are the REAL ones (measured on wolfaenpak, research/understand/real-forge-fidelity.md): `FAIL_IF_EXISTS` on
  an existing key → 409 `KEY_CONFLICT`; failed transaction condition → 400 `CONDITIONAL_CHECK_FAILED`; > 25 ops → 422
  `UNPROCESSABLE_ENTITY`; undeclared entity → 404 `SCHEMA_NOT_FOUND`.
- KVS consistency: `get` and transaction conditions strict, queries eventually consistent (§2.8 S2).

### 2.5 World-change guarantees
- Sprint closes → its ledger is final; it leaves the widget (active sprints only).
- Issue moves to another board's sprint → recorded as `removed` from the old sprint and `added` to the new one, with
  the NEW board's estimate.
- A board's estimation field changes → changes after the switch use the new field; earlier rows keep their estimate.
- Issue deleted → its rows stay as history with `deleted: true`; it no longer counts in current scope.
- A person loses browse permission → their next request shows none of that issue's rows (no stale cache).

### 2.6 Admin panel (exact visible labels; the probe finds controls by these)
`Background share (%)` (number 10-90, default 70) · `AI explanations enabled` (toggle) · `Daily AI token budget`
(number, default 200000) · `Comment group` (text; empty = everyone who can browse) · `Rotate CI secret` (button; shows
the new secret once, then `••••<last4>`) · `Migration` (read-only text `Migrated <n> of <total> v1 rows` and
`complete` when done) · `Recent admin changes` (table: when, who, what; last 20). Saving uses a `Save settings` button.

### 2.7 Web trigger
`POST` JSON `{"eventId": string, "sentAt": unix seconds, "environment": "staging"|"production", "issueKeys": [..]}`.
Headers: `X-LZ-Timestamp: <unix seconds>`, `X-LZ-Signature: sha256=<hex HMAC-SHA256(secret, "<timestamp>.<raw body>")>`
(header names case-insensitive). Reject if |now − timestamp| > 300 s. Request/response shapes are Forge's documented
web-trigger shapes (BRIEF; body is the raw string, headers are arrays of strings, response needs `statusCode`).

### 2.8 Concurrent delivery and query consistency (hardening, 2026-10-10)
Why: GPT-6.1 Sol scored 0.9618 on the gate build (seeds 0.9607 / 0.9629 / 0.9657) while the emulator delivered every
consumer invocation one at a time and answered every query fresh, so concurrent updates to the same records and
read-after-write — what Forge 1.0's transcripts showed frontier models do NOT handle — were never exercised. Forge
processes async events at least once, unordered and, without a concurrency key, unbounded in parallel (BRIEF rob#21,
rob#28, rob#30); KVS `get` is strictly consistent and `query` eventually consistent (BRIEF rob#43). Exactly these two
platform behaviours are added (NEVER OVER-ENGINEER):
- **S1 CONCURRENT DELIVERY.** Consumer invocations may run at the same time. The emulator runs up to 3 consumer
  invocations concurrently. Whenever two pending deliveries carry the same event (a redelivery or a trigger-level
  duplicate) or name the same issue, they are started together under a deterministic READS-FIRST schedule: the KVS
  layer holds each invocation's first WRITE (set/delete/transaction commit) until every invocation of that pair has
  either issued a write itself or ended — so a check-then-act sequence (get, then set) races exactly like it does in
  production, deterministically (seeded by delivery order). KVS transactions with conditions and keyPolicy
  FAIL_IF_EXISTS are evaluated against strict current state at commit time, with the measured real-Forge errors (409
  KEY_CONFLICT; 400 CONDITIONAL_CHECK_FAILED). Product-event triggers stay as today. A push's `concurrency`
  key/limit does not exempt a pair from this schedule; on real Forge it would serialize same-key events, so the
  contract states the difference (§8).
- **S2 EVENTUALLY CONSISTENT QUERIES.** A KVS query (kvs.query / entity index query) reflects a write only once it
  is 5 virtual seconds old; get (and entity get) and transaction conditions always see current state.
- **Public text.** FORGE2-CONTRACT.md §3 carries verbatim: "Forge runs consumer invocations concurrently, including
  two deliveries of the same event; here up to 3 run at once. Key reads (get) and transaction conditions are strongly
  consistent; queries are eventually consistent and may miss writes made in the last 5 virtual seconds." It states
  the exactly-once guarantees (one ledger row per change, one comment per click or double click, one deployment per
  CI event) as holding under concurrent delivery, and §8 says "A pushed event's `concurrency` key and limit are not
  applied." STARTER.md says forge-dev's `events` and `scheduled` run the same concurrency and staleness, so an entrant
  can see both on the dev site.
- **The drive (adapter, 2026-10-10).** A pair forms only from deliveries pending together, and both drives drained
  before every delivery (0 groups in 60). The site's plan marks batches with no new draw (packs unchanged): a
  duplicate with every delivery since its original, and the two halves of a permuted same-issue pair
  (`deliverNext().batchEnd`). The probe leaves the queues undrained through a batch whose last delivery comes before
  the next agenda point (hour mark, admin/person read, CI, quota draw) and that opens outside the quota wall's window
  (two partners' first requests would both meet the wall before READS-FIRST lets either store the pause); any hold
  ends at an agenda point. `forge-dev events` completes an open batch before draining. Scoring plans carry 8
  duplicate batches and 4 permuted pairs per seed. The read-after-write window is the live-UI step: the open widget
  re-reads at the app's own realtime publish, no virtual time between them. The probe records both in
  `obs.concurrency` (batches held or why not, the kit's groups with each member's originChange/originIssue,
  same_event/same_issue counts, per live change the gap between the announced write and the re-read's first query).

## 3. Packages (one owner per file; branch per package; the integrator merges)

| pkg | owns | delivers |
|---|---|---|
| P1 kit-core | `forge2/kit/lib/{runtime,proxy,emulator,kvs}.cjs`, `forge2/kit/lib/clock.cjs` (new), `forge2/kit/bin/{lint,forge-dev}.cjs`, `forge2/kit/lint-pack/**` (new), `forge2/kit/package.json` | 1.0 defects A (flag overlay `pointer-events:none`, in `bridge-page.cjs` — P1 owns it too), C (consumer `resolver:{function,method}` form), the stale trigger clock; KVS errors per §2.4; virtual time + limits per §2.2 (timers fast-forwarded in the invocation process, virtual clock reported to the emulator); the server-rule lint pack (the 6 measured refusals: two range attributes, index name < 3 or > 50 chars or bad chars, > 20 entities, `nodejs20.x`); resolver invocation with an explicit `context` (admin / non-admin / forged payload) for the probe; `@forge/react` 12.3.0 added to the kit; `forge-dev` wires the `uikit` and `ci` subcommands from P2/P3 modules |
| P2 uikit-host | `forge2/kit/lib/uikit-host/**`, `forge2/kit/bin/uikit.cjs` | run a `render: native` module's bundle in Node with the real `@forge/react` reconciler, capture every ForgeDoc (`callBridge('reconcile')`), route `invoke` to the emulator, API `render({appDir, moduleKey, context, invoke}) → {tree(), text(), findByLabel(l), setValue(l, v), click(l), waitIdle(), invokes}`; `forge-dev uikit <moduleKey>` prints the tree for the entrant. Start from `forge/spike/harness/prove-uikit.cjs` |
| P3 webtrigger | `forge2/kit/lib/webtrigger.cjs`, `forge2/kit/bin/ci.cjs`, `forge2/site/ci.cjs` | static web-trigger ingress (documented request/response shapes) the emulator mounts at `POST /x/webtrigger/<moduleKey>`; a CI sender (`forge-dev ci send …` for the entrant; a scripted scoring sequence: valid, bad signature, tampered body, stale timestamp, replay, case-varied headers) |
| P4 site-core | `forge2/site/{fixtures,rng,state,site,jql,limits,openapi}.cjs`, `forge2/site/rest/**` (except fields.cjs), `forge2/site/rate.cjs` (new) | scale to §2.3 (fixture id bands safe above 2,500 issues), the rate model §2.1 as middleware over every `/rest` route, `state` mutation API used by P5, the custom-field id per seed, v1 preload data for the KVS |
| P5 site-world | `forge2/site/world.cjs` (new), `forge2/site/llm.cjs`, `forge2/site/rest/fields.cjs` (new) | the world timeline §2.5 (scripted, seeded; dev and scoring schedules), the app field-value endpoint + issue GET exposure, the LLM script extensions for R8 |
| P6 public | `forge2/public/**`, `forge2/starter/**` (only docs inside the starter; the v1 code stays as is) | `FORGE2-CONTRACT.md` (guarantees, §1-§2 numbers; keep ≤ ~30 KB), `spec-build-forge2.md` (the prompt: 300-call budget, definition of done, bands), `STARTER.md` (incl. how v1 data is laid out and the dev tools), `RATE-MODEL.json`, `BROWSER-TESTING.md` |
| P7 scorer | `bench/score_forge2.py`, `bench/forge2-thresholds.json`, `bench/forge2_controls.py` | composition (§4), 1.0 defects B/F/G/H fixed, v1 rows adapted to scale; imports P8's rows |
| P8 checks+oracle | `bench/forge2_checks.py` (new), `bench/forge2_oracle.py` | the R1-R9 rows and the oracle for scale, world changes, migration, dosing, field values |
| P9 probe | `bench/forge2_probe.mjs`, `bench/forge2_site.py` | drives the scoring site: world schedule, CI sequence, admin/non-admin/forged calls, UI Kit lane (P2 API), boot counts (CDP network + bridge log), writes the observations P7/P8 read |
| P10 golden | `bench/golden-forge2/**` | the v2 reference app built from the PUBLIC text only (starts as v1); must score 1.000 |
| P11 harness | `bench/isolated_tiers.py`, `bench/run_build.py`, `bench/bench_budget.py`, `bench/forge2_kit.py`, `bench/release_manifest.py`, `bench/test_*forge2*` | tier `FORGE20`, flag `--forge2`, per-tier call budget (FORGE20 = 300, everything else unchanged at 150), kit materialization for the forge2 kit, `FORGE_BENCH` regex not leaking forge2 files into other payloads, the empty-entrant e2e |

## 4. Scoring (lean)
- 1.0's composition machinery with the §17.8 fixes. Weights: v1 regression rows 0.25 in total; v2 rows 0.75 (R1 0.12,
  R2 0.13, R3 0.08, R4 0.10, R6 0.08, R5 0.10, R7 0.05, R8 0.04, R9 0.05).
- Criticals (×0.6 each, fire ONLY on an observed defect, each root priced once): v1 rows lost or corrupted by the
  migration; a web-trigger write without a valid signature; an admin action by a non-admin succeeded or the CI secret
  disclosed; a hidden issue's data shown to a person who cannot browse it; a duplicate side effect (≥ 2 comments for one
  click, or duplicate ledger rows).
- Bands: lint/bundle failure → max 0.499; none of the v2 surfaces exists (no `jira:adminPage`, no `webtrigger`, no
  `scope-ledger`) → max 0.30 (v1 untouched scores ≤ 0.30, §5: the band, not the rows, holds it there).
- No wall-clock thresholds anywhere (counts and virtual time only). Harness/probe failures are never app evidence.

## 5. Tonight's gate and pilot
- Golden v2: 1.000 on 3 scoring seeds; starter untouched ≤ 0.30; one mutant per R-family loses its rows; the run_build
  empty-entrant e2e passes with `--forge2`.
- Then the pilot through run_build `--forge2` (OpenRouter): GPT-6.1 Sol first, then Claude Haiku 5.5. Publishing to the
  site follows once the desktop app ships the forge2 tier.
