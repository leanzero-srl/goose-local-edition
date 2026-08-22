# Qwen3.8-27B and LM Studio evidence register — 2026-08-23

Status: research and experiment preregistration. This file does not change an LM Studio profile,
the swarm engine, SB7, its scorer, or a running model. The current operator profile remains the
control: medium thinking, temperature 0.7, top-p 0.8, min-p 0.

## Decision

There is no evidence for replacing the current profile with one folklore preset. The current
profile is a deliberate hybrid: Qwen's official thinking profile is 1.0 / 0.95 / top-k 20 /
presence 0 / repetition 1.0, while 0.7 / 0.8 belongs to the official non-thinking profile. That
mismatch is worth testing, but it is not proof of failure. First certify what Goose actually sends
and what LM Studio renders; then compare role-specific profiles on the same tasks and seeds.

Medium thinking is the baseline worth preserving. A newly published agentic-coding comparison
reported comparable scores for medium and xhigh, while xhigh generated almost four times as many
tokens. Qwen issue 216 independently measured no empty final answers in 48 medium calls or 47 low
calls, versus 18 in 93 xhigh calls. These are stronger reasons to avoid accidental xhigh than to
change temperature.

## Evidence and limits

- The [official Qwen3.8-27B card](https://huggingface.co/Qwen/Qwen3.8-27B) defines only `low`,
  `medium`, and `xhigh`; xhigh is the default. It recommends 1.0 / 0.95 for thinking and 0.7 / 0.8
  with presence penalty 1.5 for non-thinking. It also enables preserved thinking by default and
  warns that lower per-turn effort can increase total agent time through retries.
- The [current LM Studio model profile](https://lmstudio.ai/models/qwen/qwen3.8-27b) exposes the
  same thinking defaults: temperature 1, top-k 20, top-p 0.95, with min-p, presence penalty, and
  repeat penalty disabled. UI state is not wire evidence; Goose can still omit, override, or
  mistranslate fields.
- [Qwen issue 216](https://github.com/QwenLM/Qwen3.8/issues/216) measured xhigh empty-answer
  failures after 8,294–26,137 completion tokens despite substantial output headroom. A sampled
  xhigh arm with repetition penalty 1.1 had 0/23 failures versus 8/48 without it, with similar F1.
  That is a promising recurrence treatment, not a settled default: it is one small, unreplicated
  extraction workload and does not establish agentic-code quality.
- [Qwen issue 217](https://github.com/QwenLM/Qwen3.8/issues/217) proves that sending generic
  `high` or `max` can fail the official template; adapters must translate explicitly to
  `low|medium|xhigh`. A retrying client can make that deterministic compatibility error look like
  a long generation.
- A [Qwen3.8 agentic-coding comparison](https://www.reddit.com/r/LocalLLaMA/comments/1vr4bs4/local_agentic_coding_benchmark_qwen_38_27b_in/)
  found medium the useful operating point and xhigh similar in score but nearly four times the
  generated tokens. Its public benchmark is relevant but not SB7 and does not isolate every
  sampler field.
- Reports from LM Studio users conflict. One [medium-thinking failure report](https://www.reddit.com/r/LocalLLaMA/comments/1vsscs1/qwen_38_27b_issues/)
  describes never reaching a deliverable with the official sampler and points to verifying that
  `reasoning_effort` reaches llama.cpp. Another [successful agentic report](https://www.reddit.com/r/LocalLLaMA/comments/1vt78xd/qwen3827b_has_the_highest_level_of_agency_ive/)
  uses 1.0 / 0.95 / top-k 20. Community suggestions of temperature 0.6 therefore become an arm,
  not a recommendation.
- LM Studio's [MLX agentic-workload benchmark](https://lmstudio.ai/blog/mlx-engine-agentic-workloads)
  shows that recent continuous batching can materially improve four-way short-chat throughput,
  but its four-way long-prompt result was much closer in wall time. The tested engine version,
  model, prompt length, memory pressure, and parallel setting differ from this fleet. Configured
  parallelism is not evidence of positive marginal throughput on these three hosts.

## Gate 0: prove the actual profile

For every host and role, correlate one Goose request ID with the complete request body, rendered
LM Studio prompt, loaded-instance configuration, and provider terminal event. Record engine and
runtime version, model revision and quant, native context, requested context, parallelism, KV
cache type, flash attention, MTP/speculative decoding, sampling seed, temperature, top-p, top-k,
min-p, presence/repeat penalties, reasoning effort, enable-thinking, preserve-thinking, tool
schema bytes, prompt bytes, finish reason, reasoning tokens, answer tokens, and slot release.

The gate fails if `medium` is absent, becomes xhigh, becomes generic high/max, or appears only in
Goose telemetry without changing the rendered template. It also fails if the recorded sampling
fields cannot distinguish explicit zero from omission.

## Matched sampler arms

Run these first on archived planning/detail prompts and then on fresh matched SB7 runs. Hold model
revision, quant, engine, context, seed, role prompt, tools, and preserved-thinking policy fixed.

1. Exact wire clone of the current control, including every omitted field.
2. Medium + 0.7 / 0.8, explicitly normalized to top-k 20, presence 0, repetition 1.0.
3. Medium + official thinking sampler: 1.0 / 0.95, top-k 20, presence 0, repetition 1.0.
4. Medium + community candidate 0.6 / 0.95, top-k 20, presence 0, repetition 1.0.
5. On a profile that exhibits measured recurrence, change only repetition penalty to 1.05, then
   1.1. Do not promote it from empty-answer or recurrence rate alone; task quality must hold.
6. Separately compare preserved thinking on versus off for multi-turn implementation. It changes
   both context/KV reuse and the opportunity to repeat stale reasoning, so it must not be mixed
   into the sampler arms.

Reasoning effort is a separate axis. Medium remains the main implementation/planning control.
Low may be tested for bounded summarization, extraction, or routing roles only after its output
meets the same semantic acceptance contract. Xhigh is a negative/control arm, not a candidate
fleet default, until its empty-answer behavior is disproven on our exact stack.

## Runtime and concurrency arms

Measure one versus two admitted streams on each physical host for the exact role/context regime.
Record aggregate useful tokens per second, per-request time to first token, decode rate, peak
memory, cancellations reaching provider terminal, task acceptance, and downstream retries. Repeat
for the currently loaded engine before comparing a newer MLX engine, MTP, KV quantization, or a
different quant. Never combine runtime, concurrency, and sampler changes in one attribution arm.

Parallel capacity is admitted only when useful completion throughput and quality improve. An idle
logical lane does not prove an idle decoder, and a busy decoder generating redundant reasoning is
not credited as utilization.

## Promotion rule

A profile wins only if it preserves requirement coverage and acceptance quality while improving
useful wall time or node-minutes across planning, build, and repair. Token reduction, recurrence
reduction, a single clean answer, or higher raw tokens/second is a diagnostic metric, not a win.
No hard generation cap is introduced by this experiment.
