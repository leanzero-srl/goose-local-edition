# SB7.1 pilot release review — 2026-09-20

Not release-ready. Finish and correct the recorded Gemini pilot before website work or a new release. Preserve the candidate: fixes to measurement do not authorize rewriting its implementation or manufacturing a lower model score.

## Actual run

Installed-app Benchmark launch: `cloud-b3c625d6-1c5e-4c3f-8b5b-e2b154d7ae59`, Gemini 3.8 Flash, seed `5cd00e961d80464f`. Isolated session started 16:54:41 UTC and finished 17:03:55 UTC: 9m14s model work. Scoring follows separately. Source archive and SHA manifest: `runs/sb71-validation/20260920/gemini-b3c625d6-original`.

An earlier launch was stopped for an environment error: Python could not read the actual macOS timezone symlink target. Commit b01caa495 permits only resolved existing timezone directories and tests Berlin winter/summer offsets plus continued private scorer/log denial. The completed pilot subsequently attempted Chrome self-testing, but the sandbox denied its framework. External grading of the artifact remains meaningful; do not represent this as an unrestricted model comparison. A tested, isolated browser must be available before future paid entrants, with its actual tool instructions supplied up front.

## Findings before release

1. **The visual contract limits the requested finesse.** Four exact axis-aligned solids, fixed flat shading and a prescribed collar motion test structure, not artistic excellence. The existing reference's plain appearance is not Gemini output. The pilot independently passed four-currency geometry and collar width checks. Do not pretend that simply making those geometric tests stricter creates a beautiful showcase. A revised public brief needs a deliberate payments-related design, room for lighting/material/presentation choices, and observable criteria for a polished, readable result while preserving truthful amounts, states and backend coupling. Prove a compelling reference before another paid run; retain this pilot under its original prompt hash.

2. **Stream witness selection used the wrong criterion.** The original scorer refused `fire_d1_mutation:failed`. All 101 original poses rejected the seed's target before any mutation was fired; therefore zero observed stream batches cannot be charged to the model. The picking helper's 7-pixel neighborhood and depth margin are unnecessarily restrictive for a 3-by-3 color sample. A dedicated independent pixel witness must retain identity, shading and before/after distinguishability without weakening the separate picking checks. Re-score the saved tree after actual-seed regression and negative controls.

3. **Correct version labels received zero.** All four inspector records displayed the correct identity, amount, currency, status and `v1`; the scorer demanded bare `1`, although the contract never requires that format. Share a strict whole-string semantic version parser between context and stale/duplicate checks. Accept clear labels; reject wrong numbers and contradictory text. Preserve raw text in observations. This corrects measurement without awarding unobserved animation credit.

4. **The exported video can miss the animation.** The original raw recording exists, but the public clip blindly takes its final 30 seconds. Inspected frames at 0 and 8 seconds show the restored full field, not the intended close-up animation. Record the actual inspection/motion interval and extract it from the same session; retain the original raw video and interval in the manifest. A caption must not claim an animation that the clip does not show.

5. **Run status misreports cloud execution.** After the first interrupted launch, the app said the engine never launched, despite recorded provider usage and tool writes. Its Cancel button was disabled during a running cloud task. Cloud lifecycle reporting needs actual harness/agent states and tested cancellation, rather than inferring execution solely from swarm engine events.

6. **Signed payload changes itself at runtime.** Signature verification passed immediately after installation. Importing the changed Python module then rewrote a sealed `__pycache__/bench_isolation.cpython-314.pyc`, invalidating the seal. Suppress runtime bytecode writes or put caches outside the signed bundle; verify both before and after a real harness invocation.

7. **Cost reporting is incomplete.** Session counters record 9,596,417 input tokens, 5,702,519 cached input tokens, 75,873 candidate output tokens and 9,731,669 total tokens. Google's provider adapter includes cached tokens in input but does not add thinking tokens to output. Total minus input is 135,252, leaving 59,379 tokens unrepresented in the candidate-output counter. No invoice/cost value was returned. At the official standard introductory rates on 2026-09-20 ($0.75/M input, $0.075/M cached input, $3.75/M output including thinking), the recorded-output subtotal is $3.6326; treating all non-input tokens as billable output gives $3.8553. These are reconstructions, not a verified bill. Source: https://ai.google.dev/gemini-api/docs/pricing . Do not compare them as a proven saving against the contaminated historical Gemini run.

## Decision

Correct and independently verify the scorer against the preserved candidate, then inspect its actual media and individual backend failures. Keep SB7 stable and SB7.1 explicitly a pilot. A low score is not the release criterion: faithful measurement, a genuinely stronger visual challenge, predictable cost accounting, usable submission/reporting and successful reference plus defect controls are.
