# Swarm repair causal implementation status — 2026-08-23

This note records what the `codex/swarm-repair-causal-promotion` slice implements against
`REPAIR-CAUSAL-AUDIT-2026-08-23.md`, and what remains deliberately unwired. It is an implementation
status record, not benchmark evidence. No model, scorer, publisher, website, or live run was invoked.
SB7 and its scorer have zero diff in this branch.

## Implemented engine invariants

- A degraded completion is `TaskState::Salvaged`, never `Done`. Its typed receipt retains the engine
  reason, exact SHA-256 artifact map, and required full-ruler verification. Dependents may proceed, but
  the run report and final decision retain provisional truth.
- Watchdog/stall/finalize salvage refuses owns-nothing work, tests, manifest-only work, unsafe paths,
  partial multi-file output, symlinks, empty files, and receipt drift. A deterministic judge accept may
  preserve a complete test or manifest artifact, but only as provisional input to the full ruler; its
  receipt alone never licenses green. The archived Qwen3.8 r1 `test-webhook` contradiction is a deterministic
  regression fixture.
- Complete verification emits a structured defect ledger with stable causal IDs, mechanical impact
  evidence, all file/interface/task subjects, immutable full evidence refs, tree provenance, and
  before/after deltas. Volatile ports, temp roots, traceback lines, ordering, and count churn do not
  change defect identity. Evidence history is retained when the same cause recurs.
- Applicable ruler legs are established only when they ran, inspected at least one subject, and were not
  partial. A partial scan retains its real typed findings and adds a separate unestablished observation;
  it cannot erase useful causal identity or license verification.
- `RepairTransaction` freezes the exact tree and ledger, rejects stale/foreign ledgers, unsafe or aliased
  paths, duplicate/overlapping candidates, and no-ops, composes candidates in one preview, and calls an
  async full-ruler seam suitable for the existing async gate implementation. Promotion requires target
  closure, every leg established, no introduced mechanical blocker, and a semantic acceptance receipt
  citing target evidence.
- The semantic receipt identity commits to the base epoch, target IDs, paths, modes, deletion/write kind,
  and exact bytes. Reusing a candidate display name cannot replay approval onto a different patch.
- Before the first real write, promotion freezes a full ruled-tree rollback copy. Any observed write,
  ruler, tree, or ledger failure restores and re-hashes that complete parent epoch, including files a
  misbehaving ruler created outside the candidate set. The same ruler runs immediately on the landed tree;
  preview snapshots are taken after the ruler so comparison covers the exact ruled state.

## Deliberately not claimed

The runtime has not yet transferred mutation authority to `RepairTransaction`. Legacy whole-tree race,
scheduled fix, hand-fan, unassigned join, and standalone paths still call `pick_repair_winner`,
`shard_beats_baseline`, and `promote_speculative`. Removing those before a real semantic-review receipt
producer exists would either fabricate deterministic semantic acceptance or disable repair. The next
integration slice must produce `SemanticRepairReview` from the semantic judge, translate shadow diffs into
`RepairCandidatePatch`, pass the complete failed/provisional inventory into every candidate ruler call,
route every writer through the transaction, and only then delete scalar promotion authority.

`one_ruler_grade` does call the canonical collector, but its current candidate sites pass empty failed-task
and provisional inventories and collapse the result back to `(count, established)`. It is therefore a
compatibility bridge, not proof that candidate promotion and round-open truth are identical. No benchmark
should be started to validate transactional promotion until the authority transfer above is complete and
the consolidated Rust gates pass.

## Validation boundary

Before the disk-cache cleanup, `cargo check -p goose-swarm -p goose-cli` passed the explicit salvage/source
portion, prior to the later ledger/transaction integration. After cleanup, the coordinating agent ordered
Cargo compilation and tests to remain off in this worktree. The final source received only parse/format,
whitespace, shell syntax, fixture JSON, deterministic fixture regeneration, and call-site/static scans here;
the integration worktree owns the consolidated compile, focused tests, strict Clippy, and full relevant
gate.
