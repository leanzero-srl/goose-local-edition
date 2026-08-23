# Engine control registry ledger

## Scope

This foundation change accounts for the engine controls without changing judge, planning, fanning,
repair, SB7, or scoring behavior.

The Rust source of truth is
`crates/goose-cli/src/commands/swarm_control_registry.rs`. It contains exactly 116 persisted
`SwarmConfig` controls, the audited 30/8/32/12/34 dispositions, all current environment-only controls,
canonical aliases, serialized config defaults, and the exact `GOOSE_SWARM_*` production reader names.

The task-compiler slice retired `GOOSE_SWARM_DETAIL_BUDGET_SECS` rather than leaving a catalog-only knob:
typed details are no longer cut off and replaced by one-line briefs. The current inventory therefore has 49
environment-only controls and 141 literal production readers. The bidirectional source test derives those
sets rather than preserving the former counts as inert compatibility names.

## Enforced truths

- A config field missing from the registry, duplicated in it, or replaced by a catalog-only name fails a
  unit test.
- A literal environment reader without a registry row fails, as does a registry environment name with no
  production reader.
- The obsolete comment-only `GOOSE_SWARM_REVIEW_FANOUT` and `GOOSE_SWARM_REVIEW_REPRO` names are absent from
  production source and pinned by a regression test.
- Ordinary resolution uses one tested precedence primitive: environment, config, runtime profile, default.
- `goose swarm controls` exports the same machine-readable registry without loading, probing, or calling a
  model. The export binds the build identity and a canonical SHA-256 digest to registry schema 2, plus a
  digest of every registered `GOOSE_SWARM_*` input so ambient overrides cannot change between arms unseen.
- Registry rows now carry `campaign_role` (`behavior`, `runtime_profile`, `removal`, or `telemetry`), source,
  and the value type accepted by the real `SwarmConfig` deserializer. No second type catalogue exists.
- `levers_resolved.control_registry` exports the machine-readable registry on every run and includes the same
  registry digest as the pre-run command.
- `levers_resolved.levers` starts from all 116 config fields and overlays the expressions execution uses;
  absent optional fields therefore appear as `null` rather than disappearing.
- CLI- and runtime-shaped values are echoed after resolution: actual worker devices and speed weights,
  successfully built worker extensions, parallel-planning activation, planner/worker budgets, sampling,
  the shared local-context resolver, and the shared tool-response spill threshold.
- `split_inherit_spec` is echoed by calling the scheduler's resolver. Its unset value is now reported as
  enabled, matching execution.
- The default-on environment-only paths called out by the audit (`judge`, `prereview`, `qa`, `tail_review`,
  `spec_repair`, `salvage_spin`, `ship_best`, and `sink_shard`) have effective event values. Judge,
  pre-review, ship-best, and salvage now share their resolver between the branch and the echo instead of
  duplicating parsers.
- Registry rows state whether an environment-only control has a run-level effective echo. That metadata is
  checked bidirectionally against the event; controls without one remain registered but must be read from
  their phase-specific evidence rather than being presented as run-level values.
- Uncapped runs echo the effective values actually executed: disabled sink/progress/spiral/split controls,
  expanded planner/scout/complete/draft/clarity ceilings, and expanded worker/sink turn budgets.
- `dynamic_replan` is canonical. The former `dynamic_replan_cfg` event spelling remains only as a declared
  compatibility alias.

## Campaign integration boundary

`evals/swarm-bench/bench/campaign_controls.py` is the versioned campaign consumer. It seals the exact binary,
build identity, registry digest, runtime-baseline bytes, reference profile, candidate profile, and one-control
delta before an arm may launch. It stages a config instead of touching a live config, and verifies the run's
own `levers_resolved` event afterwards. Unknown, missing, environment-only, runtime-profile, removal,
telemetry, alias collision, default-on no-op, implicit ablation, multi-control, ambient-environment drift, and
stale-binary cases fail closed. Post-run verification compares the complete executed-control projection with
a verified reference and accepts only the declared single delta (or no delta for a replicate).

The stopped external state at `~/goose-builds/loop-state` was read but deliberately not edited. Its old
`arm_config.py` remains unsafe as an authority until the isolated integration commit is merged and its
launcher adopts the staged config plus receipts. The exact migration evidence is in
`CAMPAIGN-CONTROL-HANDSHAKE-LEDGER.md`.

## Verification gate

Run:

```bash
source bin/activate-hermit
cargo fmt -p goose-cli
cargo fmt -p goose-swarm
cargo fmt -p goose
cargo test -p goose-cli swarm_control_registry
PYTHONPATH=evals/swarm-bench PYTHONWARNINGS=error \
  python3 -m unittest -v bench.test_campaign_controls
cargo test -p goose-cli config_backed_gate_sits_between_env_and_the_assured_default
cargo test -p goose-swarm tail_review_gate_defaults_on_and_respects_the_env
cargo test -p goose large_response_handler
cargo clippy -p goose -p goose-cli -p goose-swarm --all-targets -- -D warnings
```
