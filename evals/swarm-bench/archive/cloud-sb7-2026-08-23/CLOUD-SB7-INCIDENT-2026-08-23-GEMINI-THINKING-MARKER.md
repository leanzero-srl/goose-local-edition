# Cloud SB7 Gemini thinking-marker verifier incident

Date: 2026-08-23

## Classification

Infrastructure defect in the frozen smoke verifier. No full benchmark episode
started, no score exists, and no model outcome is being reclassified.

## Observed evidence

The first smoke under qualified campaign
`cloud-sb7-20260823-qualified-q1` passed for GLM-5.3, Gemini 3.7 Flash,
DeepSeek V4 Flash, and DeepSeek V4 Pro. Gemini 3.1 Pro Preview alone was marked
failed with `event 1: final marker appeared outside assistant text`.

Its sealed stream is:

`/Users/mihaiperdum/goose-builds/cloud-sb7-20260823-qualified-q1/smoke/gemini-3.1-pro-preview/attempts/attempt-1/logs/smoke.log`

The stream SHA-256 is
`90db260d8b1c06e6847f117dbe0e6018b7fbcb16f998ea778c1742b383f8f4d8`.
The corresponding attempt-evidence SHA-256 is
`84bae483f631deb81d5c4ad93795a665ddd845232b6936cb3dd8ccc5e806f6d1`.

The provider contract itself succeeded:

- exactly one extension-qualified shell request carried the frozen command;
- exactly one successful response paired by request ID and carried exact
  structured stdout plus exit code zero;
- the nonce file is regular, non-symbolic, and has the expected bytes;
- the final assistant text, concatenated in stream order after the tool
  response, is exactly the frozen final marker;
- one complete event followed the final assistant text;
- both admitted provider requests reached correlated provider-terminal states
  with usage, the ledger settled both, and no reservation remains;
- the process group, descendants, listener isolation, secret scan, and raw-tree
  isolation all passed.

The model emitted the final text in three ordinary assistant-text chunks. That
chunking already passed the verifier's authoritative concatenated comparison.
Before the tool request, however, its private `thinking` content quoted the
requested final marker while reasoning about the instruction. The verifier
searched every string in the serialized event and rejected that private
reasoning occurrence as if it were user-visible final output.

## Root cause

`parse_smoke_stream` had two independent rules:

1. concatenate assistant `text` items after the paired tool response and
   require the result to equal the final marker; and
2. reject the marker in every string path except an assistant `text` path.

The second rule treated a structured assistant `thinking` item as proof of a
premature or misplaced answer. It cannot provide that proof. Thinking is not
the final assistant-text channel, and the first rule already prevents a
thinking-only response from passing. The same marker remains forbidden in user
messages, tool payloads, metadata, non-message events, and assistant text
before the paired response.

## Correction and controls

The verifier now exempts only the exact `thinking` string of an assistant-role
`thinking` content item from the cross-channel marker scan. It does not append
that value to final text and does not relax request, response, nonce, ordering,
completion, lifecycle, budget, isolation, or exact-output checks.

The regression reproduces both material provider behaviors together: private
thinking quotes the complete target marker, and the real final answer is split
across three assistant-text events. That stream must pass. Its negative control
removes the three final-text events while leaving the marker in thinking; it
must fail because private reasoning can never substitute for the required
final answer. Existing prompt-echo, pre-response text, wrong tool identity,
wrong structured output, non-zero exit, missing completion, and truncation
controls remain failing.

Because this is a coordinator-verifier defect and the frozen Goose binary is
not defective, the ordinary supersession transition now admits an unchanged
binary only under a narrower gate: the coordinator must be the sole changed
instrument, every full entrant must still have zero episodes and zero lifecycle
events, and every raw benchmark tree must remain empty. A thinking-marker fix
therefore cannot be used to rerun a full model outcome or smuggle in any scorer,
task, manifest, provider, publisher, or binary change. The successor is still
the campaign's single allowed supersession hop and still requires a fresh
strict all-entrant smoke proof.
