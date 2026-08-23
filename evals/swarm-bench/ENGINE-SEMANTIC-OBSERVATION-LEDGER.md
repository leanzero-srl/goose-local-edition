# Engine 4 semantic observation and Engine 5 admission ledger

Date: 2026-08-23

- Implementation base: `codex/swarm-engine-overhaul@1af43ef02`
- Implementation branch: `codex/swarm-semantic-judge-observer`
- Physical broker: `c60b309b1`, merged by `7e96aafc5`
- Engine 4 protocol/control commits: `e2855e6ef`, `fafe85c66`, `94f6ec36f`,
  `2183e3b32`, `33b95b2cb`, `8a925e538`, `21437d4d7`, `b9b81f954`
- Engine 5 observation/admission commits: `b51673fbb`, `9b95c3fd6`, `824ea9a87`,
  `ccc271276`

## Hard boundary

This branch implements observation-only semantic review for the physical scheduler. It does not change
SB7, its scorer, LM Studio configuration, any benchmark, website data, or live state, and no model was
launched while building or testing it.

The legacy non-deterministic judge/nudge path is unchanged. The physical scheduler still rejects that
legacy judge because running both control paths would double-spend review capacity and leave the old
keyword parser authoritative. The new path has no conversion into `JudgeOutcome` and no API that can
nudge, stop, kill, accept, split, route, or schedule from a semantic response. Every response, including
`NUDGE`, `SPLIT_PROPOSAL`, and `ACCEPT_CANDIDATE`, ends as an observation-only receipt.

The normal path remains unchanged while `GOOSE_SWARM_PHYSICAL_BROKER` is unset. When the operator
explicitly requests the physical broker, the main execute scheduler now binds every worker provider
turn to an exact request/terminal lifecycle receipt and attaches this observation-only semantic path.
The default legacy judge, pre-reviewer, and idle replanner are substituted rather than attached beside
it; the nested omni-judge is disabled because it would start a second provider request while the worker
owns the host permit. Explicit speculative twins fail before execute because first-wins cancellation is
not yet a safe physical action.

The physical snapshot includes same-run verified nodes excluded from build by `MAX_NODES`. They remain
outside the build DAG and worker count, but can accept trace-versioned observation work through the same
broker when physically idle. This uses available machines without making hardware count author tasks.
The fix-round scheduler is still a separate legacy boundary, retains the operator's requested legacy
judge/pre-review behavior, and does not claim physical admission.

## Implemented semantic protocol

The strict protocol accepts exactly:

`CONTINUE|NUDGE|SPLIT_PROPOSAL|ROUTE_FINDING|ACCEPT_CANDIDATE|REQUEST_EVIDENCE|ABSTAIN|INCOMPLETE`.

Unknown actions or fields, prose, Markdown fences, malformed JSON, missing action-specific data, wrong
protocol, wrong snapshot hash, invented evidence, and stale results become `ABSTAIN`. Rationale words
cannot override the typed action. A nudge requires guidance; a route must name a sealed route; acceptance
must cover the exact sealed oracle; incomplete must name known unmet requirements; and a split needs at
least two grounded descriptive boundaries. A split observation is not a `TaskSpec` and cannot mutate a
DAG.

Every request carries an exact evidence-source catalog. The canonical SHA-256 snapshot seals the
task/attempt/source revision, contract and artifact versions, acceptance oracle, dependency and sibling
contract versions, allowed finding routes, complete artifact evidence, trace, and neutral measurements.
Semantically unordered collections and nested measurement JSON are canonicalized before identity is
minted.

Per-task monotonic authority rejects an older attempt/revision and rejects two hashes that claim one
revision. One asynchronous review flight is allowed per task. Identical in-flight and completed snapshots
are deduplicated; a result superseded while in flight becomes stale `ABSTAIN`. Reviewer failure or panic
becomes `ABSTAIN` only when provider terminal is established. A panic, local stream drop, or ambiguous
transport failure leaves the physical claim unresolved rather than inventing a terminal receipt.

## Production trace snapshot producer

`GooseSemanticObservationSnapshotProducer` reads
`.swarm/activity/<injectively-encoded-task-id>.json` and the exact owned-file set supplied by the
scheduler. A capture is emitted only from a typed active digest; `processing` and `done` digests,
missing digests, seed-only state, and state that changes during capture produce no summons. Partial,
unknown, model-mismatched, or internally inconsistent active digests fail closed.

The producer:

1. serializes capture for the same task while allowing different tasks to capture concurrently;
2. reads the digest and every owned artifact twice and seals only two identical observations;
3. sorts/deduplicates owned paths, rejects absolute/parent traversal, and rejects every symlink
   component before and after reading;
4. hashes complete artifact bytes, represents UTF-8 exactly, represents binary bytes as complete
   base64, and records missing artifacts explicitly;
5. returns the same cached sealed revision for identical state and increments the checked monotonic
   revision only when the typed measurement or contract/artifact identity changes; and
6. seals exactly one neutral `TraceStateAdvanced` signal into the resulting snapshot.

There is no duration, output-token, reasoning-character, recurrence-rate, review-count, or retry cap.
The 48-character shingle window and 65,536-window/history reach bound telemetry memory only; they do not
stop or alter a worker. Complete artifact capture also has no byte cap. That is intentional for this
foundation and is listed below as a real context-risk requiring evidence before any live activation.

## Neutral recurrence evidence

The stream loop now feeds every `MessageContent::Thinking` delta into a stride-one 48-character
fingerprint meter before the legacy 2,400-character tail truncation. The retained window reaches 65,536
shingles and includes an earlier excerpt 20,000-40,000 characters behind the current tail. The meter is
reset only where the existing same-session legacy nudge already resets its reasoning counters; it has no
control branch of its own.

Repeated share is defined as `(observed - distinct) / observed`. The exact preserved 9,304-byte F924
capture is stored at
`crates/goose-cli/src/commands/fixtures/f924-looping-detail-call.txt` with SHA-256
`e16c2aeecb9a847bc75b33a1194111046f53093cb915b63a7723fe44021196b3`.
The replay pins 0.4033 and separately reconstructs the withdrawn 0.6758 result from the wrong
`repeated / distinct` denominator. A chunk-invariance replay and a 2,000-step advancing-reasoning
negative guard against stream chunking and slow-healthy false positives.

Recurrence remains evidence only. It neither votes for `NUDGE` nor corroborates a semantic response.

## Physical scheduler summons and admission

The physical scheduler constructs `SemanticObservationCaptureRequest` only for an exact claimed task,
using its full contract, acceptance criteria, dependency/sibling contract hashes, owned files, downstream
finding routes, attempt, rank, and actually admitted logical device/model. Capture runs asynchronously
only while measured physical provider capacity is idle. One capture may be in flight per task.

The scheduler emits `semantic_observation_summoned` for a new sealed trace revision, excludes the
observed worker lane, intersects broker-verified lanes with the reviewer's verified provider bindings,
and invokes `submit_if_idle`. No alternate verified route/provider produces
`semantic_observation_deferred`, with no semantic admission or fabricated provider receipt. An idle
race lost to higher-priority work atomically withdraws the auxiliary opportunity; the same sealed
revision remains eligible for a later idle retry.

One consumed revision cannot be reviewed twice. Replaying the same revision/hash is a no-op; claiming
the same revision with a different hash is a capture failure. The scheduler waits for in-flight
observation cleanup before terminal run reporting. A replay in which the reviewer returns `NUDGE`
proves the build completes unchanged and no action is delivered.

## Exact broker and provider lifecycle

Each observation publishes a `TaskVersion { kind: Trace { trace_sequence, snapshot_hash } }`, registers
and revalidates it, then requests `WorkRole::SemanticJudgeObservation` at the broker-derived auxiliary
priority. Provider start is recorded immediately before the call; `Finished` or `Failed` is recorded
against the exact request key; local completion and admission release follow. Pre-call route/snapshot
rejection records `provider_not_started` and never invokes the provider.

`GooseAdmittedSemanticObservationReviewer` binds concrete Goose providers to exact
`VerifiedPhysicalLane` values. Preflight matches fleet snapshot, logical lane, model, host, model
instance, credential-free hash of the canonical provider transport endpoint, route evidence, capacity evidence,
task/attempt/revision, trace sequence, and snapshot hash. The provider exposes the actual endpoint
identity constructed by `ApiClient`; the lane carries the same SHA-256 identity observed for this run,
without serializing or logging the endpoint. Missing, mismatched, or later-drifted transport identity fails before provider start. A provider absent from that
exact binding is not eligible for scheduling and cannot start a call.

The adapter sends one user message, no tools, and a strict OpenAI `json_schema` response format. It
accepts text plus non-actionable thinking blocks, rejects tool/action/image/system content, and leaves the
existing strict semantic parser authoritative. Invalid response content observed after natural stream EOF
is passed to that parser as an abstaining protocol failure; it is not mislabeled as a provider failure.
Definitive HTTP failures may close as failed, while network loss, mid-stream error, or reviewer panic stays
unresolved and blocks replacement admission.

Adversarial review found that `Provider::stream` was not single-attempt: LM Studio resolves to
`OpenAiProvider::stream`, which internally used `ProviderRetry::with_retry`. Commit `ccc271276`
therefore adds a fail-closed `Provider::stream_once` seam. Providers default to unsupported; route
binding rejects them before admission. `OpenAiProvider` implements the separate path for both Chat
Completions and Responses without credential, network, rate-limit, or status retry. The admitted adapter
calls only `stream_once`, never `stream`, `complete`, `complete_fast`, or `ProviderRetry`.
A wiremock HTTP-500 replay expects and observes exactly one POST. The concrete adapter mock also makes
its ordinary `stream` and `complete` paths fail and proves neither is touched.

## Frozen semantic corpus

`crates/goose-swarm/tests/fixtures/semantic-judge-observation-corpus.json` carries nine cases:

1. corrected F924 long-period recurrence;
2. slow healthy reasoning that advances a falsifiable hypothesis;
3. flat coarse counters while a tool payload streams;
4. advancing but wrong interface work;
5. genuinely complete work with exact oracle coverage;
6. uncertainty caused by missing artifact evidence;
7. the historical Fable task-position correction;
8. the F163 flat-counter negative; and
9. the r1 negative-keyword parser false positive.

The F163 case preserves the 105s/174s/234s flat-counter sequence that later wrote at 294s. The historical
r1 string `VERDICT|OK|HIGH|The worker is healthy; there is no looping or drift.` proves negative
keywords cannot become an action. The useful Fable correction remains a grounded `NUDGE` label but an
observation-only receipt.

These are offline, human-reviewed labels. They prove protocol, grounding, version, deduplication,
scheduling, admission, and lifecycle mechanics. They do not prove that a local model will produce the
expected actions, improve quality, reduce time, or add node value.

## Deterministic provider-free dispatch boundary

`ProviderLifecycleDispatcher` defaults to `ProviderRequired`. Only a dispatcher that explicitly
classifies and implements `DeterministicProviderFree` can use `run_provider_free`; that path emits
`provider_free_dispatch_started` and produces no admission or fabricated provider receipt. Replay-only
`skeleton::` and `join::` fixtures prove the seam. Concrete Goose classification remains unwired
because the production dispatcher must prove the selected path cannot fall through to an agent/model call.

## Offline verification

Run from this branch inside Hermit:

```text
cargo test -p goose-swarm --test semantic_observation_control_replay --test semantic_scheduler_replay
cargo test -p goose-swarm
cargo test -p goose-providers single_attempt_stream_never_enters_the_provider_retry_loop
cargo test -p goose-cli swarm_semantic --no-default-features --features rustls-tls
cargo clippy -p goose-swarm --all-targets -- -D warnings
cargo clippy -p goose-provider-types -p goose-providers --all-targets -- -D warnings
cargo clippy -p goose-cli --all-targets --no-default-features --features rustls-tls -- -D warnings
cargo fmt --all -- --check
git diff --check
```

The Engine 5 pass is green: 9 semantic-control races, 3 scheduler replays, 9 concrete CLI
producer/provider tests, and the single-attempt HTTP retry negative. Full `goose-swarm` passed 172
tests (78 library, 6 historical judge, 18 broker, 16 physical-control, 39 scheduler, 9 semantic
control, 3 semantic corpus, and 3 semantic scheduler). Full `goose-providers` passed 83 tests and
full `goose-provider-types` passed 373. Strict relevant all-target clippy passes with warnings
denied. No command in this verification starts a model.

## Current opt-in production boundary

The main execute scheduler is production-wired only behind `GOOSE_SWARM_PHYSICAL_BROKER=1`. The same-run
snapshot must be complete, the configured endpoint's `/v1/models` response must positively list every
physical model route, every OpenAI-compatible provider route must expose the exact hashed canonical
transport, and all observation responses remain non-authoritative. No local or cloud model, scorer,
publisher, or benchmark was launched while wiring or testing this path. Offline compilation and replay
tests are not live calibration and do not justify enabling the flag in a benchmark yet.

## Exact remaining Engine 5 gap

The next integration must close these boundaries in order of correctness dependency, not implementation
effort:

1. **Live observation calibration.** Run observation-only first and measure summons cadence, idle capacity
   consumed, prompt/context size, valid/abstain rate, F924 recall, slow-healthy/F163 false positives,
   advancing-but-wrong findings, and artifact-grounding quality. The offline corpus is not a performance
   claim.
2. **Atomic evidence hardening.** Two identical reads and symlink rejection are fail-closed but not an OS
   snapshot. Activity writers are not atomic-renames, and a hostile local process can race path components
   between validation and open. A stable malformed digest currently becomes a capture failure. Resolve
   these at the real provider boundary before treating artifact evidence as security-sensitive.
3. **Context policy from evidence.** Complete artifacts deliberately have no byte cap, so a large owned
   file set can exceed the review model context and abstain. Any later excerpt/selection policy must carry
   explicit `complete: false` provenance and be evaluated; it cannot silently truncate or introduce a
   generic hard cap.
4. **`NUDGE` authority.** Ordinary guidance may be delivered only at a natural tool/turn boundary. An
   interrupt requires cooperative cancellation, the old provider terminal receipt, valid partial-session
   commit, and same-session continuation before a replacement request. Requests cannot overlap.
5. **Other action authorities.** `SPLIT_PROPOSAL` must pass the ordinary binder;
   `ROUTE_FINDING` must revalidate current ownership; `ACCEPT_CANDIDATE` must run the objective oracle
   against the same artifact hash; and `REQUEST_EVIDENCE`/`INCOMPLETE` must create separately admitted,
   contract-derived evidence work.
6. **Causal evaluation.** Compare observation-only with each intervention on matched cases, recording
   delivered guidance, next worker action, artifact/evidence delta, requirement closure, objective oracle,
   provider overlap, and decoder-minutes. Repeated `CONTINUE` and raw finding counts earn no value by
   themselves.
7. **Physical repair-round coverage.** The scheduler-backed repair round must either receive a fresh
   physical control plane with the same route/lifecycle proof or remain explicitly legacy. It cannot
   inherit a physical label from the main execute phase.
8. **Concrete provider-free classification.** Wire deterministic `skeleton::`/`join::` only after the
   selected production path is proven provider-free end to end.

No Engine 5 action authority, benchmark enablement, or performance claim is implemented or implied by
this branch.
