# Swarm work pause checkpoint — 2026-08-23

This is a deliberate offline pause. No model benchmark, hermetic scorer, publisher, or website process is running. Do not launch any local or cloud run until Mihai gives fresh explicit permission.

## Invariants that must survive resume

- The public website remains stable `sb-7.0`, not an RC label.
- The two local-fleet document IDs remain under SB7: `brun-fleet-qwen38-sb70` and `brun-fleet-qwen-sb70`.
- Do not touch the website before a real hermetic score exists and launch/publish permission is explicit.
- Never restore deterministic model kills or fixed output/time caps as a substitute for semantic supervision.
- API credentials supplied in chat are not written here or committed anywhere.

## Worktrees and durable commits

- Local integration: `/Users/mihaiperdum/Projects/goose-engine-integration`, branch `codex/swarm-provider-boundary`. Pre-checkpoint HEAD was `1fd5c37e2`.
- Cloud integration: `/Users/mihaiperdum/Projects/goose-cloud-harness`, branch `codex/cloud-sb7-harness`, clean at `985b6ff40`.
- Cloud runtime hardening: `/Users/mihaiperdum/Projects/goose-cloud-runtime-integrity`, branch `codex/cloud-runtime-integrity`. Durable commits are `402a51da9`, `f45449306`, `a879421eb`, `774a8ffa6`, and pause checkpoint `49a994ac1`. The worktree is clean and pushed; the interrupted final full-suite rerun has not certified the checkpoint.
- Repair causal promotion: `/Users/mihaiperdum/Projects/goose-repair-causal-promotion`, branch `codex/swarm-repair-causal-promotion`, clean at `7609e71a3`; cherry-pick `a2533d341` then `7609e71a3` only after the local provider-boundary slice is complete.
- Website correction is already committed separately as `694927b`; do not alter it during this implementation pause.

## Local integration state

The current checkpoint is intentionally not build-ready. It preserves the complete provider-boundary and semantic-observation work plus an adversarial hardening pass that stopped midway. Earlier focused checks passed before the latest changes, but none of those earlier results certify the current tree.

Implemented or substantially implemented:

- same-run physical transport provenance carried through fleet lanes and admissions;
- positive endpoint model membership before a configured route may become verified physical evidence;
- task-local provider lifecycle accounting around physical main-build calls;
- single-attempt provider dispatch to avoid hiding internal retries;
- unresolved occupancy on dropped, aborted, pre-stream ambiguous, and mid-stream ambiguous provider calls;
- physical main-build semantic observations without giving observations action authority;
- legacy fix-round supervision kept separate from physical main-build substitutions;
- long-reach repetition evidence and observation-only semantic review wiring;
- SHA-256-only provider transport IDs enforced at physical fleet snapshot validation;
- model-aware OpenAI endpoint identity started so chat-completions and Responses API routes cannot share a false identity;
- typed semantic-review terminal-versus-unresolved lifecycle handling started.

The exact partial seam at pause:

- `Provider::transport_identity` now takes `model_name`; production and most fixtures were updated.
- `SingleAttemptFailureProvenance` was added to the provider trait and OpenAI received a provider-owned classifier.
- The OpenAI file still needs the new type imported.
- `swarm_provider_lifecycle.rs` and `swarm_semantic.rs` still call the old generic `provider_error_proves_terminal_response`; remove that helper and use each provider's `single_attempt_failure_provenance` instead.
- Test mock providers that deliberately prove a terminal response must opt into `TerminalResponse`; all other mocks should inherit the fail-closed `Unresolved` default.
- Re-run `rg -n "provider_error_proves_terminal_response|transport_identity\\("` after completing the seam.

## Adversarial findings still to close

1. Provider failure provenance must remain provider-owned. A shared `ProviderError` variant is not generic proof of a remote terminal response. Defaults must stay unresolved.
2. `ApiClient::transport_identity` must include the actual `default_query` in the hashed URL and return `None` when an opaque request-builder decorator could mutate the URL. Update its credential-safety test: a query changes the digest but never appears in serialized evidence.
3. Physical semantic supervision depends on `.swarm/activity` freshness. A startup write probe alone is insufficient because later ENOSPC/I/O errors are currently discarded. Do not simply propagate `?` from inside an active provider stream: that can drop an unproven remote decode and intentionally wedge its permit. Introduce a persistent, observable activity-sink health state so later write failures make semantic supervision explicitly degraded/unavailable without fabricating a provider terminal.
4. Keep physical-main supervision substitution scoped to main execute. Planning, research, and legacy repair/fix rounds retain their existing judge behavior until their own physical lifecycle is proved.
5. Repair promotion commits provide provisional/salvaged state and causal receipts, but no producer yet emits cited `SemanticRepairReview`; legacy scalar promotion and whole-tree fan/join/ship-best paths are not yet transactionally converted. Do not claim live parity.

## Resume sequence

1. Confirm the launch gate is still closed and inspect all four worktrees before editing.
2. Finish provider-owned failure provenance and model-aware endpoint identity, including query/decorator fail-closed tests.
3. Design and implement persistent activity-sink failure evidence without terminating an unresolved provider request.
4. Run `cargo fmt`, `git diff --check`, targeted provider/lifecycle/physical-broker/semantic tests, then `cargo check -p goose-providers -p goose-swarm -p goose-cli`.
5. Commit and push the completed local provider-boundary slice.
6. Cherry-pick `a2533d341` and `7609e71a3`, resolving `swarm.rs` by preserving both lifecycle and causal-repair changes; then run focused and consolidated gates.
7. Resume the cloud runtime worktree at `49a994ac1`, inspect the checkpoint diff, complete its interrupted full suite, and add any required correction commit; then cherry-pick the five runtime-hardening commits onto `codex/cloud-sb7-harness`.
8. Run final offline clippy/test gates and adversarial review. Do not start models.
9. Clean only exact regenerable target directories. The first cleanup already reclaimed about 131 GB; at pause the local integration target was about 16 GB and the data volume had about 114 GiB free.
10. Report readiness and wait for Mihai's explicit permission before launching either local or cloud benchmarks.

## Pause verification

All three worker/reviewer agents were interrupted. A process scan showed no Cargo build/test, cloud harness test, benchmark, scorer, publisher, or model process owned by these worktrees.
