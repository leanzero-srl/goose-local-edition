# SB7.1: requirements and measured cost audit

Status: design in progress, 2026-09-20. No SB7.1 model run has started. SB7 remains the default. SB8 artifacts are retained as experiments; its 100% score is not evidence of SB7-equivalent difficulty.

## User requirements

1. Reduce model completion time and monetary cost. The user reports $100–150 for two frontier-model SB7 runs; this is the budget problem to solve, not a verified invoice total in this audit.
2. A more complex backend-driven 3D element, with greater emphasis on visible structure and animation.
3. More discriminating, judicious scoring of correct and well-presented 3D output. Actual screenshots, detailed observations, and scoring explanations must remain part of the product.

The exact visual subject is pending clarification: a more detailed articulated gantry versus a richer version of the SB7 payments field. Do not implement a replacement subject without resolving that distinction.

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

The primary session contains 586 unique shell tool-request IDs. Raw console marker counts are unreliable: later commands read the console itself, causing earlier commands to appear again in output.

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
- Structural and motion checks must observe geometry and pixels: connected moving parts, dimensions, depth/occlusion, backend-commanded pose, load attachment and time-series trajectory. A changed screenshot alone is insufficient animation evidence.
- Score readability and presentation with transparent, reproducible criteria and reference/mutant comparisons. Avoid hardcoded aesthetic preferences or a model judge being the sole authority.
- Preserve SB7's detailed per-check evidence, root-cause attribution, severity explanation and screenshot viewer. Keep the original SB7 scorer and results unchanged.

## Acceptance before claiming a cheaper, stronger benchmark

A correct reference must pass; structurally wrong, visually misleading, uncoupled and static-animation mutants must fail for their actual defects. Freeze the candidate contract before a paid run. Compare fresh isolated entrants on model time, actual usage and correctly rendered product evidence. Cost reduction and frontier difficulty remain unproven until those measurements exist; neither altered weights nor a lower score alone establishes them.
