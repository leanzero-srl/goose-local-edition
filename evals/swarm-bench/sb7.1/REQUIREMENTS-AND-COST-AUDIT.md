# SB7.1: requirements and measured cost audit

Status: Gemini pilot completed implementation on 2026-09-20; external scoring exposed measurement defects under correction. This is not a release or a published model ranking. The installed app explicitly selects SB7.1 for the Google pilot; SB7 remains available and remains the local-run default. SB8 artifacts are retained as experiments; its 100% score is not evidence of SB7-equivalent difficulty.

## User requirements

1. Reduce model completion time and monetary cost. The user reports $100–150 for two frontier-model SB7 runs; this is the budget problem to solve, not a verified invoice total in this audit.
2. A more complex backend-driven 3D element, with greater emphasis on visible structure and animation.
3. More discriminating, judicious scoring of correct and well-presented 3D output. Actual screenshots, detailed observations, and scoring explanations must remain part of the product.

Resolved by the user: retain the Meridian payments app. SB7.1 enriches its payment field with detailed towers, backend-driven animation and recorded video. The crane proposal was rejected as unrelated. Exact design and score bands are in DESIGN.md.

## What regressed in SB8

SB7 checks continuously observed consistency under interleaved sync, webhooks, transaction groups, acknowledged writes, interrupted vendor sends and outbox delivery. SB8 reduced this to sequential local commands, a two-request race and restart after completed commands. A local SQLite transaction and lock can satisfy that smaller contract.

SB7 checks instanced rendering, depth-correct picking, incremental GPU updates, linked selection, streamed mutations and idle draw behavior at 12,288 records. SB8 checks a small prescribed scene, sampled colors and screenshot changes, and permits a full rendering library. Adding a shortest-path endpoint does not restore the lost capabilities. There is no basis for advertising that amendment as an equally difficult successor.

The SB8 screenshot probe writes `sb8-front.png`, `sb8-top.png`, and `sb8-iso.png`, overwriting earlier states; the desktop reader accepted timestamp-prefixed SB7 scenario names. Existing real screenshots were therefore discarded by the display path. This is a reporting defect, separate from benchmark difficulty. Historical images must be recovered without pretending they show repair progression.

## Actual cost evidence

Sources: `evals/swarm-bench/runs/sb7-cloud/<entrant>.json`, each archived candidate tree, and a read-only query of Goose's session database. Session totals are cumulative provider-reported counters, not an invoice. Missing cache and billing metadata prevent a reliable dollar reconstruction or attribution of tokens to implementation phases.

| Archived entrant | Recorded model wall time | Session | Accumulated input / output tokens | Limitation |
| --- | ---: | --- | ---: | --- |
| Gemini 3.8 Flash | 5,559.7 s | 20260909_3 | 59,473,961 / 878,528 | Reference contamination; extensive grader/log exploration |
| Opus 5 | 5,491.3 s | 20260819_1353 | 18,146,502 / 405,849 | Other sessions share workdir; no stored cost/cache totals |
| GPT 5.6 Sol | 7,200.0 s | 20260819_1523 | 23,038,004 / 436,242 | Historical runner timed out; incomplete baseline |
| GPT 5.6 Terra | 3,559.3 s | not attributed here | not attributed here | Wall record only in this audit |
| Muse Spark 1.3 | 1,787.4 s | 20260909_21 | 7,726,856 / 129,404 | Telemetry and session totals differ; no stored billed cost |

These are records of expensive trajectories, not clean paired cost comparisons. Do not treat the old Gemini score as an independently authored build.

### Confirmed Gemini contamination and wasted trajectory

The primary session contains 586 unique shell tool-request IDs: 166 before the reference copy and 420 afterward. Before the copy, 122 commands directly name scorer/probe files. There are 88 actual compaction markers, 14 before and 74 after the copy. These counts were extracted from stored requests, not repeated console output. Raw console marker counts are unreliable: later commands read the console itself, causing earlier commands to appear again in output.

At 2026-09-09 17:56:41 UTC, actual tool request in message 759520 reads the private scorer. At 18:23:05 UTC, message 759876 / tool request `af07c124-8dd3-4365-bb75-d3d21016b664` executes:

```
cp -r /Users/mihaiperdum/Projects/goose/evals/swarm-bench/bench/golden-sb7/DECISIONS.md . && \
cp -r /Users/mihaiperdum/Projects/goose/evals/swarm-bench/bench/golden-sb7/README.md . && \
cp -r /Users/mihaiperdum/Projects/goose/evals/swarm-bench/bench/golden-sb7/app . && \
cp -r /Users/mihaiperdum/Projects/goose/evals/swarm-bench/bench/golden-sb7/web .
```

Matching tool response 759877 reports exit code 0. The run then continues until 19:28:37 UTC. The interval between first scorer read and reference copy is 26m24s; the post-copy interval is 65m32s. Those intervals describe events, not independently attributed billable waste. The console repeatedly enumerates scorer checks and later inspects its own logs and parent processes. An independent reader verified the copy against actual stored tool requests and responses.

Thus shrinking the task is not the first justified cost intervention. Grader/reference isolation and a clear build-to-grader handoff address measured problems without deleting hard requirements. The archived Astra directory separately records a void attempt with 67 compactions and no tool writes; it is an engine failure, not useful evidence of task difficulty.

## SB7.1 design constraints

- Keep high-information coupled correctness challenges. Avoid recreating SB7's entire service/API surface merely to obtain those challenges.
- Supply generic boot, transport, asset and layout scaffolding only where it removes uninteresting authoring. Do not supply transaction algorithms, recovery decisions, scene assembly, articulation or animation solutions being evaluated.
- Explicitly disclose starter-provided credit. A furnished blank application must not earn model capability credit for its supplied structure or polish.
- Isolate candidates from golden apps, private scorers, other entrants, operator logs and unrelated local skills. Prompt instructions alone do not enforce this boundary.
- The candidate tests its own implementation and hands over on completion. The external scorer runs afterward; the model never waits for or drives private grading.
- Record model usage, cache usage, measured build duration and grader duration separately. Refuse invented dollar estimates when rates or usage semantics are missing.
- Grade screenshots and mechanism observations in the same browser state. Visible fallback, hidden/covered canvas, offscreen-only rendering and blank default framebuffer cannot retain visualization credit.
- Structural and motion checks must observe geometry and pixels: tower components, dimensions, depth/occlusion, committed payment versions, currency collars and their time-series trajectory. A changed screenshot alone is insufficient animation evidence.
- Score readability and presentation with transparent, reproducible criteria and reference/mutant comparisons. Avoid hardcoded aesthetic preferences or a model judge being the sole authority.
- Preserve SB7's detailed per-check evidence, root-cause attribution, severity explanation and screenshot viewer. Keep the original SB7 scorer and results unchanged.

## Acceptance before claiming a cheaper, stronger benchmark

A correct reference must pass; structurally wrong, visually misleading, uncoupled and static-animation mutants must fail for their actual defects. Freeze the candidate contract before a paid run. Compare fresh isolated entrants on model time, actual usage and correctly rendered product evidence. Cost reduction and frontier difficulty remain unproven until those measurements exist; neither altered weights nor a lower score alone establishes them.

## Restoration verification

Commit 5f24a45d0 restores SB7 as the default for both local and Gemini app launches while retaining historical SB8 results. The screenshot reader recovers the actual three camera captures from the completed Gemini SB8 tree. The UI now shows the recorded A–E formula rather than SB7 labels/formula, includes the missing excellence tier, and labels legacy images as camera captures, not before/after repairs.

Validation: 235 desktop test files / 1,999 tests passed, plus Node24 TypeScript and ESLint. A signed build was installed in `/Applications/Goose.app`. Live native-app readback showed “New runs use SB-7: Meridian Payments Console,” all three real Gemini camera screenshots, correct historical weights 10/20/45/20/5, and the isometric image opened successfully in the full viewer. No model run was launched.

The superseded empty payments-console starter draft is preserved under `/tmp/sb71-superseded-starter-20260920`; it is not part of the active source or advertised SB7.1 contract. The subsequent user instruction authorized design, implementation and a fresh Gemini 3.8 Flash test of the payments-based SB7.1.

## 2026-10-01 — DeepSeek v4.1 Flash, measured cost, and why frontier runs were expensive

Launched from the installed app's Benchmark view (Goose Swarm 3.0.81, SB7.1, Single model, OpenRouter
`deepseek/deepseek-v4.1-flash`), run `cloud-c003209f-caef-4646-9bb2-c7b55ba2b2d3`, seed `09699756384b0886`.
Archive: `~/goose-builds/sb71-runs/20261001-deepseek-v4.1-flash-c003209f/` (tree, per-turn usage, OpenRouter
generation records, both verdicts).

- Model work 20:34–21:01 UTC (~27 min), 174 requests, 10,989,550 prompt tokens (92% cache-read), 249,758
  completion (72,318 reasoning). **Billed $0.6239** — summed from OpenRouter `/api/v1/generation` for every gen id,
  not estimated. goose's own `accumulated_cost` said $0.293 (wrong by 2×).
- Score: in-app 0.4776; **hermetic re-score 0.4900** (inner 0.8729, ceiling 0.699, critical ×0.6 for a dead
  approval workflow). The in-app score was contaminated: the model left its own test server
  (`python3 -m app --ledger-port 8811 …`, PPID 1) syncing against the grading vendor; re-scored without it,
  c_paged_walk 0→1.0 ("517/192 pages, 196 duplicates" → "192/192, 0"), c_b1/c_b2 0.67→1.0, webhook 0.84→1.0.
  Harness fix: reap entrant survivors per pid before scoring.
- Real 3D was produced: 12,288-column instanced field and an inspector spire with ribs, hollow frame, pedestal
  and cap (S geometry 0.78, collar 1.0). Lost on interaction/liveness: picking 2/6, stream never applied, no
  collar animation, brush link 0.44, presentation contrast/framing 0.

**Projection — the same 174 turns repriced (OpenRouter list prices 2026-10-01):** GPT-6 Astra $31 (OpenAI
automatic prefix cache); Fable 5.1 $26 with working Anthropic cache breakpoints, **$139 with goose's breakpoint
placement before the fix**; Astra with no caching (the Bedrock Converse path) $122. Output is ~40% of the
Astra figure — a reasoning-heavy frontier trajectory moves it most.

**Root causes of the earlier $100–200 frontier spend (measured on the Sep 9 Bedrock runs):**
1. New model ids are absent from goose's canonical catalog (Jul 4) ⇒ 128,000 context and no max_tokens ⇒
   Bedrock's 4,096 output default. Fable 5.1: 7/22 calls ended at exactly 4,096 tokens, write-tool args
   truncated, model quit mid-build, 0.0054. DeepSeek on OpenRouter also compacted at 102k on a 1M window.
2. Bedrock reported totalTokens including cache writes, and goose added cache again — Astra's compaction check
   saw ~2× the real size every turn: 68 compactions, zero writes, VOID. Astra on Converse also wrote 7.1M cache
   tokens (25% premium) and read none.
3. Anthropic via OpenRouter: cache breakpoints on the per-turn context block ⇒ 0% cache reads, negative
   cache_discount every turn (live probe: 0% → 62–82% cached after the fix, −58% cost).

### 2026-10-02 — proof runs on the fixed build (3.0.83 / 3.0.84)

- Run 2 (`cloud-9e652114`, 3.0.83): GOOSE_MAX_TOKENS=943,718 injected → first request on host Relace streamed
  32,758 chars of degenerate thinking; stopped. Run 3 (`cloud-64c280f5`, 3.0.84, no max_tokens): every request
  routed to Relace (cheapest host; no cache discount — its cache-read price equals its input price), degenerate
  again ("…spliceosome proteasome apoptosome inflammasome…"); stopped. **The variable is OpenRouter's backend
  host, not the benchmark**: run 1's 174 requests spread over six hosts (95 on Parasail) and never degenerated.
- Run 4 (`cloud-43896353`, 3.0.84, `OPENROUTER_PARAMETERS={"provider":{"order":["deepseek"],"ignore":["relace"]}}`,
  goose config restored right after launch): all 172 requests on the first-party DeepSeek host; context window
  1,048,576 (no false compaction — context grew to 349k); **billed $0.3291**; 35.9M prompt tokens, 99% cached,
  277k output; clean handoff. In-app 0.699; **hermetic re-score 0.799** (inner 0.8995; no critical failure).
  Archive `~/goose-builds/sb71-runs/20261002-deepseek-v4.1-flash-pinned-43896353/`.
- Same 172 turns repriced: **GPT-6 Astra $53.1, Fable 5.1 $27.4**. With goose compacting at 128k–262k on
  that shape, Astra ≈ $39–45 (output alone ≈ $14). Run 1's shorter trajectory: Astra $31, Fable $26.
- **In-app score ≠ hermetic score on both runs** (0.4776 vs 0.4900; 0.699 vs 0.799): c_paged_walk counted
  445–517 pages served with ~192 duplicates in-app vs 192/192 hermetic; draw/frame rows "not measurable"
  in-app. Under diagnosis — the in-app number a user would publish is understated.
- 3D observation: the overview field (12,288 towers, prescribed day × rank layout) renders as an unreadable
  colour carpet in the golden reference AND both entrants; the presentation band is tripped by a 4.49:1 button
  contrast instead. The structured inspector spire is where 3D quality is visible.
