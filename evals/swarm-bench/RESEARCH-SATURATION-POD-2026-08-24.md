# Research saturation pod checkpoint

## Measured defect

Local SB7 run `swarm-20260823-221953474` completed nine capacity-derived seed packets and six evidence lookups with all three physical LM Studio nodes participating. At `2026-08-23T22:50:58Z`, the engine then admitted one `evidence-saturation-coordinator` on `workhorse-qwen/qwen3.8-27b` for all 197 immutable requirements. Mihai and Gabee remained idle while that call exceeded 74,000 reasoning characters and 8 MB of provider stream data. Its full-stream 48-character recurrence share stayed below 0.07, so this was not a repetition incident. The work unit itself was monolithic: the model had to re-audit and serialize every requirement into one typed ledger.

This distinguishes the defect from a timeout or token-cap problem. Stopping that call would discard legitimate coverage work; waiting for it leaves two thirds of the physical fleet unused.

## Implemented behavior

Branch `codex/research-saturation-pod` replaces the single model-owned ledger call with a capacity-derived pod:

1. Authored semantic boundaries are partitioned by the same cost-aware splitter used by the seed fan. The desired queue depth comes from distinct hosts plus configured execution slots and is bounded only by the number of requirements.
2. Each packet contains disjoint immutable requirement authority and only evidence records bound to those requirements. If a large authored section is split for load balance, every sibling requirement in that section is included as read-only context so local cross-references are not severed; the compiler still forbids output authority for context-only ids.
3. Packets work-steal across distinct physical hosts and retry a failed typed packet only on a different host.
4. Every packet is independently compiled against its exact requirement and evidence authority. Invalid-ledger correction still terminates on repeated canonical semantic state, not time, tokens, or attempt count.
5. The engine—not another model—restores authored requirement order, namespaces locally generated question IDs, applies the existing global blocked/continue/saturated rules, and recompiles the merged ledger against the complete authority set.

The final ledger remains singular and deterministic; only the expensive semantic audit and serialization are distributed. Planning still receives research only after every packet crosses the barrier.

## Safety and falsifiers

- No SB7 scorer, specification, website document ID, sampling setting, or hard cap changes.
- A packet cannot cite evidence bound only to another packet's requirements.
- A global merge fails on omitted, repeated, or invented requirements and on repeated semantic evidence slots.
- A blocked packet remains globally authoritative; runnable sibling questions are not dispatched after the existing blocked terminal condition.
- The monitor rejects saturation before the seed merge, duplicate initial host assignments, and retry on a previously failed host.
- The change is falsified if a new run still starts one all-requirements saturation activity, if initial saturation admissions do not span the available distinct hosts when enough requirements exist, or if the merged ledger differs from the immutable requirement set.

## Verification at checkpoint

- `cargo check -p goose-cli`: pass.
- `cargo test -p goose-cli research_saturation`: 7 passed.
- `python3 -m unittest -q scripts.tests.test_monitor_swarm_run`: 20 passed.
- Live v6 run remains untouched; this branch is for the next validated binary.
