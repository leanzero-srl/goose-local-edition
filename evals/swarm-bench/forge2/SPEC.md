# Forge 2.0 — Scope Ledger 2, repaired benchmark specification

Current engineering specification, 2026-10-11. Public task requirements live in `public/FORGE2-CONTRACT.md`, the rate model in `public/RATE-MODEL.json`, and the entrant prompt in `public/spec-build-forge2.md`. Those files, the starter, SDK kit and scorer are fingerprinted together. Prior specifications and scores remain in git and archived run evidence; they do not define this repaired task.

## 1. Task and capability coverage

One model inside Goose upgrades the working Scope Ledger v1 app to v2, with 300 model calls, offline development tools and a seeded dev Jira site. No reference implementation, scorer, scoring seeds or previous solution is exposed. The product remains a sprint-scope ledger, rather than a collection of unrelated coding puzzles.

| Family | Required outcome | Weight |
|---|---|---|
| R1 | Preserve all v1 row data, leave v1 intact, migrate exactly once during live delivery, resume across limits, finish within two virtual hours, show progress | 0.12 |
| R2 | Complete background work within the 70% hourly share, reserve quota for people, honor Retry-After and each reason for throttling | 0.13 |
| R3 | Continue work across invocation limits and concurrent redelivery without partial duplicates | 0.08 |
| R4 | Correctly handle closed sprints, cross-board moves, estimation-field changes, deletion and permission loss | 0.10 |
| R5 | Working native UI Kit admin panel with server-side authorization, settings, audit history and secret rotation | 0.10 |
| R6 | Signed CI web trigger with timestamp checks and atomic replay handling, including concurrent identical requests | 0.08 |
| R7 | Correct custom-field values within the same virtual hour | 0.05 |
| R8 | Actual successful Forge LLM explanations with validated tools, backoff, complete answers, cache, kill switch and token budget | 0.04 |
| R9 | Custom UI boot within one invoke, 150 KB and the declared origin restrictions | 0.05 |
| R10 | Native Jira `scopeChanged(sprintId)` IN/NOT IN queries, correct changed-issue membership, shared precomputation reuse, backend refresh and argument refusal | 0.10 |
| v1 regression | The existing Scope Ledger product, UI, permission, Rovo, idempotency and economy guarantees continue to work | 0.15 |

R10 uses a real `jira:jqlFunction` declaration, actual handler execution, native-search expansion and the JQL precomputation REST API. A manifest stub earns only its declaration row. Initial, cached, refreshed, per-user, empty, future, closed and invalid-argument results are compared with independent fixture facts. Native Jira permission filtering is applied after shared expansion; an incorrect shared result is not itself evidence of a hidden-data disclosure.

## 2. Platform model and disclosed assumptions

The pinned SDKs, manifest linter and bundled OpenAPI are used in development and scoring. Platform research and the causal audit are in `repair/RESEARCH.md`.

### 2.1 Scale and current quota semantics

Each scoring site contains three projects, four scrum boards and a kanban board, six active, two future and six closed sprints, 960–1,040 issues, pre-upgrade ledger data and six virtual hours of changing work. Relevant and irrelevant updates, dropped and duplicate deliveries, permission changes and world events are seeded. The dev site exposes the same behavior classes with different identities and schedule.

The normalized installation budget is 2,400 points per clock hour and the default background share is 70% (1,680 points). This is a workload assumption, not an Atlassian production entitlement. It makes expensive reads and progress-preserving dosing consequential without changing the actual published cost classes.

Reads cost one base point plus returned top-level domain objects; identity/access objects cost two each. Semantic reads such as POST search, bulk changelog fetch and precomputation search remain reads. Mutations cost one base point. Nested fields do not multiply object counts. Request burst buckets are separate from quota points. GET/POST refill at 100 requests/s, PUT/DELETE at 50, issue GET at 150 and changelog GET at 200. The one-second initial buffer and PATCH at 50 are disclosed benchmark assumptions. Per-issue successful writes are limited by both 20 writes/2 seconds and 100 writes/30 seconds.

Structured RateLimit headers report the hourly global or tenant policy; X-RateLimit headers report endpoint request capacity. Both global and tenant quota refusals occur across fixture seeds. The full shapes and reactions are public in RATE-MODEL.json; the scorer observes actual charged points and retry behavior. Its write-only lower bound is not a proof of workload feasibility: the known-good runtime control must finish under the share limit.

### 2.2 Virtual runtime, storage and concurrency

UI resolvers and JQL functions have 25-second virtual limits. Consumers and scheduled functions default to 55 seconds and support the declared timeout up to 900 seconds. Web triggers and Rovo actions have 55 seconds. Proxied requests and in-function waits advance virtual time. No score depends on host wall-clock speed.

Key reads and conditional transactions are strongly consistent. Index queries have a disclosed five-virtual-second lag; production Forge does not promise that exact duration. Transactions enforce the real operation limit, entity schema, conditional-check and key-conflict behavior.

Consumer concurrency keys are honored installation-wide across queues. Same-key limit-one work is serialized. Unkeyed work can run concurrently, with up to three harness workers. Seeded reads-first interleavings exercise duplicate and same-issue deliveries reproducibly. Concurrent CI replay uses actual simultaneous handler invocations and observed ledger writes. Safe atomic code is allowed to prevent the race; the harness does not disable the protection in order to produce failures.

### 2.3 Current modules and API semantics

The workload covers Custom UI, native UI Kit, dashboard/sprint surfaces, Rovo actions, custom fields, product/lifecycle/scheduled triggers, async events, KVS custom entities, Forge LLM, web triggers and custom JQL. Each functional requirement is exercised through its module or HTTP boundary. Preview modules that the offline runtime does not implement are not advertised as tested capabilities. This benchmark cannot certify every Forge module or production deployment behavior.

## 3. Scorer and evidence

There are currently 105 scored checks: 67 v1 regression checks and 38 v2 checks, grouped into 19 non-economy reliability groups plus economy. The public task bytes fix separate development and scoring seed domains before any model observations. Every independent build on those task bytes receives the same development fixture and three common scoring fixtures, for six virtual hours each. Ordinary rows keep their worst fixture value; economy rows keep their mean. The complete per-fixture rows and observations are retained. Numeric worst-case selection never erases measured critical harm from another fixture: all positive harm evidence is retained and each class is priced once. A missing or lower-scoring surface on another site cannot suppress that harm; a harness gap still makes the result unpublishable.

Earned points use the weights above. Five severe defect classes each multiply by 0.6 at most once: migration data lost/corrupted, unsigned web-trigger writes, non-admin changes or secret disclosure, hidden data disclosed, and duplicate side effects. These penalties require observed harm. Failed happy paths, absent surfaces, denied requests and invalid declarations do not suppress harm that actually occurred elsewhere in the same run. A missing valid explanation cannot earn cache credit merely because it made zero calls.

For each non-economy group, reliability contributes `1 - 0.10 * (1 - worst eligible row)`, with a combined floor of 0.25. Critical rows with an observed defect are excluded from reliability to avoid pricing the same critical twice. This is an explicit policy penalty, not a statistical confidence interval. Composition must be monotone with critical state held fixed; the selftest checks that property. Lint/bundle failure caps at 0.499; no v2 surface caps at 0.30. Existing continuous cap behavior is retained.

An idle or missing app receives no vacuous success credit. Missing observation evidence caused by the harness makes a run unavailable for publication, rather than a fabricated model failure. Provider, credit, budget and infrastructure interruptions are likewise unscored. A completed 300-call session can be scored as the submitted artifact.

## 4. Free qualification and task freeze

Before paid pilots: meaningful synthetic scorer controls; actual SDK/runtime tests; full known-good execution; unchanged starter and empty entrant; deliberately broken implementations for each requirement family; repeated scoring of an identical tree; and fresh reference economy calibration on five distinct seeds. The reference must meet every non-economy guarantee and the fixed one-invoke UI budget, with complete typed rows and stored composition that exactly reads back from those rows. The five fitting sites must reach calibrated economy 1.0. The frozen event/reconcile tops remain fixed on heldouts: their actual request-efficiency losses remain scored and disclosed without pretending that a finite fit promises perfect economy on every workload. Unavailable, vacuous, held or otherwise unpublishable references refuse qualification; a generous economy threshold cannot conceal incorrect product behavior. Free single-site controls retain browser screenshots and all scored observations without encoding report-only video. Normal published builds keep the recording. Freeze task, starter, kit, scorer, thresholds and control hashes before examining new model results.

The comparison pilots are MiMo v2.6 Flash and GPT-6.1 Sol through OpenRouter and the installed Goose Benchmark view. Two independent build episodes per model use the same frozen task, common scoring seed schedule and settings. Sol scoring 0.90 or higher means the task must be deepened prospectively with another demonstrably relevant capability requirement before the final campaign. No score weights or outcomes are tuned to force a ranking. Any task change invalidates earlier pilot comparability.

## 5. Noise, comparison and cost policy

Fixture variation and stochastic build variation are separate. Re-score a preserved tree to measure harness repeatability without purchasing another model build. For the comparison, retain each independent build's score, scenario/group evidence, severe defects, completion state, cached-token receipts and cost. Report episode range and median; overlapping or contradictory episodes do not establish a winner. Two episodes are a screening pilot, not a broad model-ranking confidence claim. Additional targeted repeats require both a concrete uncertainty to resolve and available budget.

The existing 26-model roster is retained verbatim. New spend is capped at $10 for pilots including actual Goose tool-loop caching probes and $75 for the entire repaired campaign, independent of credit top-ups. Count billed generations, not merely a balance delta. Check cache behavior in actual multi-call Goose sessions and monitor the actual run receipts. Prompt caching reuses identical input prefixes, never stored model answers or another entrant's solution. Run one paid entrant and one heavy scoring process at a time, preserve artifacts and avoid paid rebuilds for scorer-only repairs.

Old Forge 2.0 results are invalidated and archived. Current board results must match the repaired frozen fingerprint. The public era remains Forge 2.0; historical and repaired artifacts are never mixed into one ranking.
