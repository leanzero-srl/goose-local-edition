# Cloud SB7 one-time orchestrator recovery

Date: 2026-08-23

## Incident boundary

The only admitted source is
`/Users/mihaiperdum/goose-builds/cloud-sb7-20260823-gemini-fixed-s1`.
It is a stopped generation-1 campaign. The operator stopped it because the
detached monitor rejected valid first-episode progression with exactly this
single log line:

```text
cloud campaign lineage refused execution: unstarted entrant acquired or reset attempts: deepseek-v4-flash
```

The manager log is empty. No entrant committed `BUILD_COMPLETE`, a score,
publication evidence, an exit code, a raw-tree outcome hash, or a model-quality
failure. All five entrants are stopped at full-episode ordinal 1. The source
ledger has upper-bound settled spend `$0.66728891` and exactly five outstanding
reservations. Each reservation equals the one non-terminal lifecycle request
for its provider/model. The recovery carries those reservations permanently;
it never manufactures provider-terminal usage and never reuses episode ordinal
1.

The read-only source audit binds these exact root artifacts:

- campaign `866c4549f3a6f4e78b776f75f587807f6416ae81bf8bfec481a9424da13bb2e7`
- budget ledger `9cf6a9b8055f7695372ded91edfad938bdd4d7b64bd2dcea042205c843482757`
- manager state `2b3fd49acdcbfb2affcd8e82cb181d5e8573d65ef8efbe8ef0b76ea359dd6722`
- monitor state `e41e0d58ce45a42193dca4ae53c18545ff5b4fc1c82a4c9fd53b10375f3ddefa`
- empty manager log `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
- monitor log `f163693a7b857a9afa54f7e18a71c7f021d500bf214ad6d7383cef93eab74bf5`
- generation-1 lineage `487001786b3a2de9b59374fd5fe35245bf811d4c5f1574677741522aff1eefd5`

Per-entrant lifecycle and exact partial-tree hashes are written into the
generated defect-evidence JSON. The coordinator also contains a closed
one-time incident identity for this exact absolute source root, campaign,
binary, manifest, smoke contract, instrument set, and the canonical digest of
the complete evidence tuple. A structurally identical campaign at any other
root is refused. The canonical frozen evidence digest is
`a0b9a7d847deb4481217f317b6d23d2208a06116661cd9402de80d5552ab64d8`.
The measured lifecycle hashes are
`ed186b464431f18f408e35c1ab6fefcfb27ab1ccdd68a768c294d334ac1082a4`
(GLM-5.3),
`0da4a92b7c2652c3bfc457145fa6eaae112eea5a2d2af318d7335fea3efee250`
(Gemini 3.7 Flash),
`ebbe4b2f7c30707923053034283720184cc91a6b48407b5ee7c0547436319335`
(Gemini 3.1 Pro),
`2fb03de0cf6c48932254697e85ba6e550d5a3008b4c95982143c2b8272d7ba6a`
(DeepSeek V4 Flash), and
`2e0870db229ac90305f1db0c5e657db73fb7c8838cd00c0bf8d01168a949a91a`
(DeepSeek V4 Pro).

## Recovery state machine

`unstarted_after_infrastructure_defect` now means that the predecessor attempt
count was zero, not that the successor must remain forever at zero. A pristine
successor may progress from attempt 0 through the frozen maximum. Resetting the
counter to zero after any lifecycle event, prompt, command, or start timestamp
is rejected.

The exceptional recovery is a distinct generation-2 transition, not a second
ordinary supersession. It is available only when the exact monitor line,
stopped process identities/groups, five attempt-1 states, absence of outcomes,
one ambiguous request per entrant, exact ledger correlation, remaining budget,
source generation, current coordinator-only code delta, evidence schema, and
artifact hashes all pass. It writes the evidence bundle, source seal, and
exclusive source receipt before constructing a sibling target in a temporary
directory. The final root rename is atomic. Repeating the command resumes the
same transition; another target, another evidence file, a second recovery, or
any later outcome is refused.

The target has fresh empty raw trees and fresh smoke states, but retains fixture
seeds and `provider_episode_attempts=1`; its only full run therefore becomes
episode 2. The cumulative ledger is copied byte-for-byte, including all five
ambiguous reservations. The source receipt makes the stopped root permanently
non-runnable. The generation-2 target cannot be superseded or recovered again.

## Evidence schema and commands

The evidence JSON has an exact, closed schema: `schema_version`,
`classification`, `defect_id`, `summary`, `source_root`, `target_root`,
`source_campaign_id`, `source_smoke_contract_sha256`, `source_binary_sha256`,
`source_instrument_set_sha256`, `target_instrument_set_sha256`,
`source_campaign_sha256`, `source_budget_ledger_sha256`,
`source_manager_sha256`, `source_monitor_sha256`, `manager_log_sha256`,
`monitor_log_sha256`, `entrants`, `fix_source_commit`, and `artifacts`.
`classification` must be `orchestrator_monitor_defect`; `artifacts` must contain
one non-empty, secret-free `root_cause` file and one `regression_test` file.
The read-only generator calculates every state, lifecycle, exact-tree, log,
ledger, source-root, target-root, instrument, artifact, and commit binding. It
makes no provider request and mutates no campaign:

```bash
python3 evals/swarm-bench/bench/cloud_sb7.py orchestrator-recovery-evidence \
  --from-root /Users/mihaiperdum/goose-builds/cloud-sb7-20260823-gemini-fixed-s1 \
  --root /Users/mihaiperdum/goose-builds/NEW-RECOVERY-ROOT \
  --root-cause /ABSOLUTE/PATH/root-cause.txt \
  --regression-test /ABSOLUTE/PATH/regression-test.txt \
  > /ABSOLUTE/PATH/orchestrator-recovery-evidence.json
```

After independent review, the one mutating transition is:

```bash
python3 evals/swarm-bench/bench/cloud_sb7.py orchestrator-recovery \
  --from-root /Users/mihaiperdum/goose-builds/cloud-sb7-20260823-gemini-fixed-s1 \
  --root /Users/mihaiperdum/goose-builds/NEW-RECOVERY-ROOT \
  --defect-evidence /ABSOLUTE/PATH/orchestrator-recovery-evidence.json \
  --publish-live
```

The transition itself makes no provider request. The target must then pass a
fresh all-model smoke. Launch order is atomic and monitor-owned:

```bash
python3 evals/swarm-bench/bench/cloud_sb7.py smoke \
  --root /Users/mihaiperdum/goose-builds/NEW-RECOVERY-ROOT
python3 evals/swarm-bench/bench/cloud_sb7.py monitor-start \
  --root /Users/mihaiperdum/goose-builds/NEW-RECOVERY-ROOT
```

Do not run `start` first. `monitor-start` validates lineage synchronously before
detaching. The detached child proves parent PID 1 and matching PID/process
group/session identity, commits `RUNNING`, and only then starts the manager from
its monitor tick. Direct manager launch fails closed unless that exact monitor
state is already live and bound to the current smoke contract.

## Regression coverage

The warning-as-error cloud harness suite passes all 111 tests. Recovery tests
cover exact attempt/ambiguity/budget conservation, reset detection, idempotent
resume, fork refusal, build-outcome refusal, accounting drift, evidence tamper,
crashes after the source receipt and final root rename, a failure midway through
evidence copying, refusal of a structurally valid source lookalike,
monitor-first manager gating, and synchronous lineage refusal before monitor
detachment.
