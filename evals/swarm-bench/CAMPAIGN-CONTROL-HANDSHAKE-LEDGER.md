# Campaign control handshake ledger

## Decision

The stopped all-lever queue must not be resumed as written. Its attribution shape is superficially correct
(two reference replicates plus 26 rows that each remove one token), but those tokens no longer describe the
engine. The replacement has no Python lever allowlist. Before a campaign writes or launches anything it asks
the selected engine binary for `goose swarm controls`, seals that binary and manifest, separates causal
behavior from runtime profile/removal/telemetry, and proves exactly one executed control changed against a
verified reference run.

No fleet, model, LM Studio process, benchmark, scorer, SB7 artifact, website, or live loop-state file was
started or changed for this audit.

## Complete current inventory

The engine export at base `1af43ef02` has registry digest
`68c756c5d0d717a6792007548332374872a827f164433c8aad471391d84167ed` and accounts for every row:

- 116 persisted controls: 69 behavior, 34 runtime profile, 12 removal/merge, one telemetry (`occupancy`).
- 49 environment-only controls: 26 behavior, 15 runtime profile, eight removal/merge.
- Total: 95 behavior, 49 runtime profile, 20 removal/merge, one telemetry; nine canonical aliases and 141
  literal production readers.

The complete row set is the schema-2 `control_registry` emitted by the engine and declared in
`crates/goose-cli/src/commands/swarm_control_registry.rs`. Recopying 165 names into this document would create
the stale catalogue this change removes. Rust tests derive all 116 fields and all 141 readers from production
source and fail on either a missing or an inert registry row. The Python consumer independently rejects
duplicate/missing canonicals, dead or colliding aliases, malformed roles/types/sources, and a bad registry
digest.

## Legacy evidence census

Every line of the following stopped-state inputs was scanned read-only. Hashes make this audit reproducible:

- `arm_config.py`: `105a7bdf2666c1ea5866a0b848744120f913ebe0af7aade5984e2a69565f5f48`
- `LEVERS`: `a93b8fcc2a0091289bd64bd1216f8ae18761390e9376fe685954b286fcd18dca`
- `ALL-ON.env`: `da759a7b560d66ca2bac7be2665ff496b4610a63f939cd52f9a46f8f67d63868`
- `QUEUE`: `12725cdee1e632e6017bd9292c94db08b3fc6498f3d74e3009da2ab0eef61d6d`
- `QUEUE.bak-pre-mtp`: `07e4faba50d8dfdb7b613d480792a36ba230f11ca7496041ab5f6c8af1e41848`
- `QUEUE.hold`: `f3fa96caff565df732a1061c82b41a31f512914dfd0c7d3e7e9020ba11d8b8a3`
- `QUEUE.rebuild-backup`: `99da9bbd17a39a916cfc6f020a8c62f0f1aa78b73e3f39a2fd1d0b96ba084622`
- `LEDGER.tsv`: `ca36e0b06de7a789d63d62d53780b883703ed1d289b32155b1a71e9d3cb1c5d9`
- `FINDINGS.md`: `1d163a5f98e74fba5c922bd5bc96d20e8a5b9b018729de5db80570226b36d86c`
- `KNOB-VALUE-ANALYSIS-2026-07-23.jsonl`:
  `70e5d00e41ccd839d76173867dfa59e4cc19fdd18f13400b8636176a2ee18ca8`

The live Python catalogue has 87 alleged settable names. Two are absent from the engine
(`repro_demotes_verified`, `review_repro`), it omits 31 real persisted controls, its only alias is one of the
engine's nine aliases, all six `APP_FORCED` names are now persisted/configurable, and two of its three
`ENV_ONLY` names (`ask_replan`, `complete`) are now persisted. An allowlist comparison cannot repair this;
the allowlist itself has been deleted from the versioned replacement.

The 32-token `ALL-ON.env` reference contains 26 behavior controls, two runtime-profile controls
(`ask_floor`, `retarget_rounds`), two removal controls (`detail_memo`, `spiral_break_chars`), and two names
that do not exist. Of its 26 behavior values, only `review=true` differs from the current engine default.
The other 25 reassert defaults.

That default fact invalidates the queued ablations more deeply than the stale names do. Omitting a key now
restores the baked `SwarmConfig::default()`, not false. Of the 26 nominal ablation rows:

- 23 are no-ops because the omitted target already defaults true (this includes retired `detail_memo`);
- two target nonexistent controls;
- only `abl-review` actually changes a current behavior value.

Consequently `allon-2` and `allon-3` are replicates of “current defaults plus review,” not an all-on engine.
The queue's text-level one-token deltas are real, but 25 of 26 do not produce an engine-level causal delta.

`LEDGER.tsv` has 20 rows, all above the fleet-model-swap boundary; there is no post-swap baseline or result.
Four rows are explicitly void, one never finished, 17 are uncrunched, and only three carry external spec
totals. It records 18 unique lever names, including the same two nonexistent names. Twelve rows claim
verified, while the durable findings document repeatedly proves model/engine claims are not external
evidence. This ledger remains historical evidence, not a source for current defaults or control existence.

All 5,121 lines of `FINDINGS.md` were scanned against the 165 canonical names and environment spellings. It
lexically mentions 88 and never mentions 77; absence is not a negative result. The admissible conclusions are
method-level: one-vs-one outcome comparison is statistically dead, a same-binary replicate moved 8/15 to
13/15, FIRED is not CORRECT, run events outrank config labels, and a default/transport change is a
comparability boundary. The July 23 knob analysis has seven analyses plus one synthesis over an older five-run
corpus. It preserves useful bottleneck evidence (serial sink tail, planning/clarify tax, idle nodes), but its
control values are superseded: for example it treats `sink_cap_secs=0`, `split_fat=false`, and `occupancy` as
current causal candidates, whereas the authoritative registry now reports a runtime profile, a modified
default-on behavior, and telemetry respectively.

## Handshake and resumability

`campaign_controls.py` implements three persistent boundaries:

1. `lock` invokes the selected binary with no timeout and no fleet access, validates all registry metadata,
   optionally matches an expected build SHA, hashes the binary bytes, hashes the value (including absence) of
   all 141 registered `GOOSE_SWARM_*` inputs, and atomically seals the export. The run captures this digest at
   function entry, before it bridges config values or creates its telemetry path. An existing identical lock
   resumes; a changed path, binary, build, schema, registry, or control environment refuses.
2. `prepare-arm` resolves aliases from the manifest, accepts only persisted behavior controls, overlays each
   profile on engine defaults, and requires exactly one delta (or exactly zero for a declared replicate). It
   also requires the changed candidate value to be explicit, so omission can never masquerade as disabling a
   default-on or nullable control. It hashes the untouched runtime baseline, strips inherited behavior keys,
   writes explicit values—including `false` for a default-on ablation—to an isolated staged config, and
   atomically seals an arm receipt.
3. `verify-event` requires one `levers_resolved` event from that build and registry, requires every registered
   effective echo, verifies every explicitly requested behavior value, and atomically records the event-log
   hash plus the complete executed-control profile. A causal result is accepted only when that profile differs
   from a verified reference by the declared control and nothing else; a replicate must have no executed
   delta. An engine claim or arm label cannot substitute for this check.

There is no model-time, token, turn, queue-length, or retry cap in this handshake. Runtime-profile values are
held by an exact baseline hash and cannot be smuggled into a causal behavior arm.

## Regression evidence

- `cargo test -p goose-cli swarm_control_registry`: 19 passed.
- `PYTHONPATH=evals/swarm-bench PYTHONWARNINGS=error python3 -m unittest -v bench.test_campaign_controls`:
  19 passed, including unknown/missing/alias/default-on, ambient-environment, staged-file tamper,
  executed-reference, and stale binary/build cases.
- A real debug binary's `goose swarm controls` export round-tripped through the Python validator with all
  165 rows and the digest above. This command did not inspect or contact LM Studio.

Confidence is high for catalogue elimination, attribution checks, and binary/manifest sealing. Confidence is
also high for the external adoption seam after the follow-up implementation below. Confidence remains lower
for the first real activation because the installed DMG has not yet been rebuilt with this engine command and
the deliberately stopped live campaign has not been migrated or exercised.

## External loop-state adoption (2026-08-23)

`campaign_runtime.py` now owns the transaction around the three handshake boundaries. Queue generation asks
the sealed manifest for every persisted behavior control, writes a complete explicit reference profile,
automatically inverts booleans (including default-on controls), and requires a preregistered alternative for
every non-boolean behavior control. It emits a complete inventory that classifies config and environment
controls as behavior, runtime profile, removal, telemetry, or unreachable from persisted config. The queue is
immutable JSONL, hash-bound to the campaign plan, and written last; it is never popped. A crash during
generation resumes only if every already-written byte is identical.

Activation performs the heartbeat check twice: once before waiting for the mutation lock and once while
holding it. It then re-verifies the binary, build, registry, environment readers, baseline, spec, queue, and
arm receipt. Before replacing global `config.yaml` it writes an activation intent and an exact backup. A crash
before or after the global write therefore resumes the same receipt; another arm cannot acquire the active
lease. Unexpected global-config drift is refused unless explicit same-arm crash recovery is requested, in
which case the observed digest is preserved before the sealed staged config is restored.

Verification now gates measurement and rotation. A ledger row is appended only after one terminal
`run_finished` and one valid `levers_resolved` prove the complete executed profile against the verified
reference. Ledger writes and release are idempotent across crashes. Static `ARM_LEVERS`, mechanism-event
maps, `ALL-ON.env`, and whitespace queue parsing are absent from the adoption source; mechanism correctness is
reported unassessed when the engine does not export a firing predicate rather than being guessed from a stale
Python map.

The actual tracked loop-state sources were changed only in a separate worktree on branch
`codex/schema2-adoption`; the live campaign directory and its dirty state were read but never modified. Both
healthy rotation and empty-state cold start use one queue-launch helper; crash recovery calls the same
receipt-bound `launch.sh`. Repeated crashes and any verification failure stop with the receipt intact rather
than skipping the arm. The old queue/catalogue files are retained only as explicit `RETIRED` tombstones.

Additional regression evidence:

- `python3 -m unittest bench.test_campaign_controls bench.test_campaign_runtime`: 30 passed. These include
  process races, heartbeat TOCTOU injection, crashes on both sides of global config replacement, idempotent
  resume/release, stale binary, queue tamper, unknown/missing/alias/default-on controls, and global-config
  divergence.
- `python3 -m unittest -v test_schema2_harness.py` in the isolated loop-state worktree: six passed. These
  prove both daemon paths use the schema-2 seam, mutation follows activation, STOP is not silently removed,
  static executable catalogues are gone, missing runtime fails closed, and both shell files parse.
- No fleet, model, LM Studio process, benchmark, scorer, website, or live campaign state was started or
  changed by these tests.
