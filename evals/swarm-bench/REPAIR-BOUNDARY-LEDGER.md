# Repair phase boundary ledger

Updated: 2026-08-23  
Branch: `codex/swarm-repair-overhaul`  
Base: `10ab496465a7b9cdff94298ec64cde8e8f87c03e`

## Scope of this increment

This increment fixes only the proven post-gate mutation defect. It does not change SB7, any scorer,
semantic-judge policy, salvage state, causal promotion, provider routing, LM Studio, or benchmark data.

The archived Qwen r1 evidence is pinned in
`fixtures/qwen38-r1-post-gate-mutation.json` (source run-log SHA-256
`6402923479726a0a1533493955c0b5625caa59661db630e7b903d274dfcdd5b6`). The last full
`complete_verify` occurred at `2026-08-22T12:26:22.908462Z`. Boot repair then changed three source
files, `complete_result` was emitted, and wire-fix subsequently changed another three source files
before `run_finished`. Three unchanged files are included as negative controls. The fixture carries
the rsync-mtime caveat; hashes and event order are the primary evidence.

The ignored archive remains at
`evals/swarm-bench/runs/sb7-fleet38/swarm-3node-r1`; the surviving nested `run.jsonl`, best-tree
snapshot, and shipped-tree paths are recorded explicitly in the fixture. The fixture hashes were
rechecked byte-for-byte against all six mutations and all three controls on 2026-08-23.

## Enforced boundary

The COMPLETE sequence is now:

1. The ordinary fix loop performs provisional checks.
2. `OpenRepairTree` records the app-tree hash and opens an epoch.
3. The loop's final full-ruler decision is bound to that epoch before ship-best selection. A
   ship-best restore is a `repair_candidate` and invalidates the loop ruling when its bytes change.
4. The functional floor then probes the bytes that can actually ship. Each boot-repair or wire-fix
   dispatch is a `repair_candidate`. A byte change advances the epoch and invalidates any prior
   ruling; a byte-identical candidate leaves it intact.
5. `run_complete_ruler`—the same collector used by the loop and speculative promotion preview—runs
   after the restore and every floor repair. This is the sole final authoritative decision.
6. `OpenRepairTree::seal` consumes the open state only when its current bytes match that authoritative
   ruling hash.
7. `SealedCompleteTree::emit_final_events` re-hashes once and emits `complete_result` and
   `run_finished` back-to-back from the same method. Both carry the SHA-256 of the exact application
   tree; there is no call-site seam for a future writer between them. Any prior byte drift emits
   `post_seal_mutation`, aborts, and emits neither terminal event.

The tree digest is language- and layout-independent. It hashes sorted relative paths, entry kinds,
directory/file modes, file bytes, symlink targets, and empty directories. Its exclusions are exactly
the pre-existing F886 ship-best `rsync` evidence exclusions (`.swarm`, `run.jsonl`, `bench-shots`,
`heartbeat`, `graded.db`). It deliberately has no language, framework, cache, or application-file
allowlist: anything that ship-best can copy participates in the ruling.

## Event contract

- `repair_tree_opened`: initial epoch, entry count, and tree hash.
- `repair_candidate`: cause, before/after epochs and hashes, exact changed paths, and whether bytes changed.
- `repair_tree_ruled`: ruler cause, epoch/hash, pass/verified bits, and remaining finding count.
- `repair_tree_sealed`: the immutable result token used by finalization.
- `complete_result`: includes `tree_hash` and `tree_epoch` from that token.
- `run_finished`: includes `complete_tree_hash` and `complete_tree_epoch` when COMPLETE ran.
- `post_seal_mutation`: sealed/current hashes and changed paths; finalization fails closed.

`complete_verify` now declares `authoritative`, `reason`, and `tree_hash`. Repair-loop checks are
provisional. `post-repair-floor` is authoritative and occurs only after ship-best restore, boot
repair, and wire-fix are all behind the boundary.

## Regression gates

Targeted unit module: `commands::swarm::repair_tree_seal_tests`.

- Any path, byte, symlink-target, empty-directory, or executable-mode change that ship-best can copy
  alters the digest; only the engine evidence excluded by the existing ship contract does not.
- A changed candidate invalidates the current ruling and cannot seal.
- A byte-identical candidate preserves the ruling (negative control).
- A mutation outside a registered candidate blocks ship-best selection and sealing.
- A sealed tree can emit a result only while its bytes still match; later drift fails closed.
- The archived r1 fixture pins six changed files, three unchanged controls, and the defective event order.
- Event-order assertions pin open/candidate/rule/seal/result/post-seal semantics.

Required local gates use the shared target only:

```text
CARGO_TARGET_DIR=/Users/mihaiperdum/Projects/goose-engine-overhaul/target cargo test -p goose-cli repair_tree_seal_tests --lib
CARGO_TARGET_DIR=/Users/mihaiperdum/Projects/goose-engine-overhaul/target cargo clippy -p goose-cli --all-targets -- -D warnings
```

Do not expand this increment into salvage or causal promotion. Those start only after this boundary
and its archived regression fixture remain green.
