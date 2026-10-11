# Repaired Forge 2.0: installed-app unattended campaign

This controller is prepared for the frozen repair, not authorized to launch until the owner’s free reference/mutant controls pass and the replacement installed app and website fingerprint are confirmed. No model calls were made while preparing it. Free script controls do not prove the installed UI or actual provider cache path; those are verified after installation and in the billed support probes.

## One durable campaign, one installed release

Use `/Users/workhorse/goose-builds/forge2-repair-campaign` for this entire repair, including prospective task deepening. Never run `init` on another directory to replenish the pilot allowance. The original credit ceiling, every generation bill and prior frozen epochs remain in the same ledger. `$10` includes both models’ support probes, four independent pilot builds, auxiliary titles and failed calls. `$75` includes all campaign calls and never resets. Current account credits and key limits can only lower those allowances. There is no auto-top-up or key mutation.

Do not resume `sb72_chain.py`, `forge2_pilot.sh`, an old Forge2 gate, or the `sb72` queue. The old chain clears the wallet and automatically publishes; this controller does neither. Keep `/Users/workhorse/goose-builds/loop-state/sb72-chain.HOLD` in place. A scored build starts only by the installed Benchmark view’s actual Run benchmark click. The controller never invokes a scorer or publication endpoint.

The owner’s release step supplies two private JSON receipts in `/Users/workhorse/goose-builds/forge2-repair-campaign-inputs`:

* `release.json`: `appVersion`, full 40-character `sourceCommit`, `engineSha256`, `taskFingerprint`, recorded from the clean release source and installed bytes. `taskFingerprint` is SHA256 of the **raw** installed `forge2/release-manifest.json`, not its parsed/reserialized JSON. The source receipt is release-process evidence, not a claim that searching binary strings proves its provenance.
* `registration.json`: the active website readback with `scorerVersion:"forge-2.0"` and `releaseManifestSha256` equal to that raw manifest SHA. The release step verifies the actual website response before saving it; an invented local registration file is not website evidence.

The installation check hashes every payload member pinned by the manifest, both receipts, the actual engine, and the executable support files. It also verifies installed Info.plist version and that both engine and payload belong to the same app bundle. Any mismatch holds before another paid unit. Preserve the old app/results separately; do not overwrite candidate trees with the reference implementation.

## Prepare on the workhorse, without buying a call

Open a login shell (`ssh workhorse`, then `zsh -l` if necessary). After the authorized release restart, the app must be `/Applications/Goose Swarm.app`, CDP9897, with no active entrant or scorer. If it is already open, do not open a second instance. The release operator starts the absent app with:

```sh
open -n '/Applications/Goose Swarm.app' --args --remote-debugging-port=9897
```

Set task-specific shell variables; do not alter HOME or CODEX_HOME:

```sh
FORGE_OPS='/Users/workhorse/goose-builds/forge2-repair-ops'
FORGE_PYTHON='/Users/workhorse/Library/Application Support/Goose/benchmark/runtime/python/bin/python3'
FORGE_NODE='/Users/workhorse/Library/Application Support/Goose/benchmark/runtime/node/bin/node'
FORGE_STATE='/Users/workhorse/goose-builds/forge2-repair-campaign'
FORGE_INPUTS='/Users/workhorse/goose-builds/forge2-repair-campaign-inputs'
```

The installed benchmark payload intentionally does not ship `repair/`. Keep this controller in the private external directory above. Export the exact committed support files from the release source; do not point this directory at an editable checkout or copy an old campaign directory. The owner supplies the verified source checkout and `release.json` before this export. This reads Git objects only and does not modify the installed app or start a call:

```sh
FORGE_SOURCE='/Users/workhorse/Projects/goose'
"$FORGE_PYTHON" - "$FORGE_SOURCE" "$FORGE_OPS" "$FORGE_INPUTS/release.json" <<'EXPORT'
import json, os, re, subprocess, sys
from pathlib import Path
source, support, release = map(Path, sys.argv[1:])
commit = json.loads(release.read_text())['sourceCommit']
if re.fullmatch('[0-9a-f]{40}', commit) is None:
    raise SystemExit('Full verified release source commit is required')
files = ('campaign.py', 'cache_probe.py', 'dispatch.mjs', 'ops.py', 'ROSTER.json', 'test_ops.py', 'RUNBOOK.md')
contents = {name: subprocess.check_output(['git', '-C', str(source), 'show', commit + ':evals/swarm-bench/forge2/repair/' + name]) for name in files}
if support.exists() and any((support / name).exists() and (support / name).read_bytes() != data for name, data in contents.items()):
    raise SystemExit('Existing private support differs; recover its original campaign before replacing it')
support.mkdir(parents=True, exist_ok=True, mode=0o700)
os.chmod(support, 0o700)
for name, data in contents.items():
    path = support / name
    path.write_bytes(data)
    os.chmod(path, 0o700 if name in ('campaign.py', 'cache_probe.py', 'dispatch.mjs') else 0o600)
print('Private committed controller, exact roster and free controls exported; no generation requested.')
EXPORT
"$FORGE_PYTHON" -m unittest discover -s "$FORGE_OPS" -p test_ops.py -q
"$FORGE_NODE" --check "$FORGE_OPS/dispatch.mjs"
"$FORGE_NODE" "$FORGE_OPS/dispatch.mjs" inspect
```

The frozen config pins all five executable/config `SUPPORT_FILES`, including `ROSTER.json`, in this external directory. `test_ops.py` and this runbook are copied from the same committed source for review. The executable harness, engine and task remain bound to the actual installed app through `load_installed`; the external controller cannot substitute its own scorer or task.

`inspect` only reads the actual form and saved app sessions. It makes no host test or generation request. The model form must be the installed Forge2 view; native effort is medium, native call budget300, OpenRouter, Any host, and all editable sampling fields use native defaults. Full builds retain the current native context treatment and send no added output cap. A blank wallet is never accepted for launch.

Create the initial frozen config only after the receipts above exist. The following reads credentials through the installed harness and requests **only** current account/key metadata. It never prints credentials, raw key metadata/labels or model prompts. A current credit or key-limit error refuses preparation. OpenRouter documents account credit readback as requiring a management key; an inference key returning403 is an unavailable check, never zero credits or authorization to guess. Resolve credential support using existing approved credentials before launching; do not create or alter keys.

```sh
"$FORGE_PYTHON" - "$FORGE_OPS" "$FORGE_INPUTS" "$FORGE_NODE" <<'PY'
from pathlib import Path
import sys
sys.path.insert(0, sys.argv[1])
from ops import SUPPORT_FILES, live_credit_evidence, load_installed, read, sha, write
support, inputs, node = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
bundle = Path('/Applications/Goose Swarm.app')
payload = bundle / 'Contents/Resources/swarm-bench'
engine = bundle / 'Contents/Resources/bin/goose'
release, registration = inputs / 'release.json', inputs / 'registration.json'
receipt = read(release)
credit = live_credit_evidence(load_installed(payload, engine))
write(inputs / 'initial-credit-readback.json', credit)
config = {'app_bundle': str(bundle), 'app_version': receipt['appVersion'],
          'source_commit': receipt['sourceCommit'], 'payload': str(payload), 'engine': str(engine),
          'engine_sha256': receipt['engineSha256'], 'taskFingerprint': receipt['taskFingerprint'],
          'node': node, 'python': sys.executable, 'release_receipt': str(release), 'release_receipt_sha256': sha(release),
          'registration_receipt': str(registration), 'registration_receipt_sha256': sha(registration),
          'support_sha256': {name: sha(support / name) for name in SUPPORT_FILES},
          'starting_credits_usd': credit['effective_remaining_usd'],
          'initial_headroom_usd': '0.50', 'qualification_wallet_usd': '0.50',
          'billing_lag_calls_reserved': 2}
write(inputs / 'frozen-config.json', config)
print('Private frozen config and current credit readback saved; no generation requested.')
PY
"$FORGE_PYTHON" "$FORGE_OPS/campaign.py" init --state "$FORGE_STATE" --config "$FORGE_INPUTS/frozen-config.json"
"$FORGE_PYTHON" "$FORGE_OPS/campaign.py" preflight --state "$FORGE_STATE"
```

The explicit50-cent qualification wallet is a stop allowance, not an expected spend or guaranteed sufficient budget. A support probe interrupted by its wallet remains unscored; never automatically restart it. The scripts preserve the native six-call requirement and report the absence. The release/operator may select another explicit qualification allowance **before** initial config, within the unchanged cumulative `$10/$75` and current credits; no replay is authorized by that choice.

## Paid execution after freeze

```sh
"$FORGE_PYTHON" "$FORGE_OPS/campaign.py" detach --state "$FORGE_STATE" --execute-paid
```

The detached controller and native benchmark processes survive SSH disconnection. Its exclusive local lock permits one owner. It first performs each pilot route’s **six actual Goose calls** through the installed `run_build.invoke`, isolated native developer tools and public Forge references: five separate real shell/tool results, then one final report of their actual hashes. These are provider/cache support probes, with no scorer, capability score or publication. Their title calls are auxiliary, billed separately, and do not count as an entrant’s seventh call.

Both pilot routes need native cached-input hits recorded by their generation bills. The controller then clicks four full Benchmark builds in roster order: MiMo1, Sol1, MiMo2, Sol2. Each receives the same frozen public task, common native medium/default settings and common hidden scoring-site schedule, with a separate candidate workspace/runtime/history. Prefix billing/KV caching can reduce spend. Replayed model responses or another candidate’s code/history cannot stand in for an actual call. Requested model IDs come from the actual outgoing native request body; displayed model labels are insufficient.

Every generation records requested ID, actual billed model/provider, native prompt/completion/cache counts and actual billed usage. Only exact requested pilot IDs or their current primary catalog `canonical_slug` are accepted as serving identity. Legitimate Jev/Pareto/Terra routing retains the requested router and actual underlying served model separately. No `~` stripping or substitution is performed; an absent owner ID remains unavailable. Each remaining exact roster route receives its own actual six-call support probe before a full build; two independent builds are targeted only when its measured projection and available credits permit them.

A complete full build has its own cache evidence, independently of support probes. Missing/invalid cache counts or no observed full-build cached input hold further paid work for free diagnosis. There is no invented cache-ratio cutoff. Single-build results remain provisional. `pilot-comparison.json` preserves the two-model screening result; `comparison.json` expands as other models finish. Both require the actual launch taskFingerprint/buildId and canonical `comparisonCohortSha`/`cohortCanonicalJson` from the installed comparison module. No winner claim is manufactured from a close or noisy spread.

Sol completing at or above0.90 permanently blocks further launches on that frozen task. Deepen the public task prospectively, qualify controls again, register/install a fresh fingerprint, and use `refreeze` on **the same** state/ledger. Existing paid evidence remains archived with its original fingerprint; changing scoring weights after seeing it cannot satisfy this gate.

## Economic guard and its actual limits

The native WalletGuard polls **ended-call telemetry** and retries late native bills while the entrant may advance. One in-flight call is therefore not a proven overrun bound. Before each unit, the controller reads current credits and key limits, subtracts existing campaign/pilot bills and reserves actual observed request envelopes, at least two delayed completed-call envelopes, and auxiliary requests. JSON request bytes/native output parameters and current advertised uncached/write prices inform the active-call screening reserve; ended native prompt/completion counts inform pending completed bills. No model context or output treatment is reduced to make the reservation pass.

During a run it retains every observed generation, delayed bill and pre-ID request envelope, including titles. It only finalizes generation bills after end telemetry or teardown. Native spend plus unresolved/in-flight reservations crossing the cumulative allowance cancels the **owned original** app run through its native Cancel dialog, or stops every verified private qualification descendant by PID. Identity/accounting failures invoke the same owned stop. Saved candidate files, original launch identity and the ledger remain. The unit is unscored. If native UI transport fails, the fallback verifies the exact installed runner argv, original cloud runId/model/tier and previously attested descendant argv hashes, then stops those owned PIDs. A transport/ownership stop failure remains explicitly `stopping-unscored`; it cannot authorize another paid launch and must be recovered before proceeding.

This is a stop ceiling plus conservative observed reserves, **not a hard billing guarantee**. Future contexts, provider tokenization/omitted output caps and billing lag can exceed the observations. Current prepaid credit and key limits add external containment; no top-up is performed. Unresolved bills or missing coverage are lower bounds, never free calls. All support calls, titles and failed generations count; a cache discount is never guessed from SDK metadata.

## Observe, stop and recover without duplicate paid work

```sh
"$FORGE_PYTHON" "$FORGE_OPS/campaign.py" status --state "$FORGE_STATE"
"$FORGE_PYTHON" "$FORGE_OPS/campaign.py" stop --state "$FORGE_STATE"
```

`stop` sets a durable stop-between-units marker and lets the current unit/billing/scoring finish. It does not cancel or restart a model. Reconnection requires only `status`; never click Run again because the SSH session disappeared. The normal detached command can resume an already recorded active original unit, with the same lock/runId/workdir. An armed dispatch without one provable original launch is ambiguous and refuses another paid start.

Completed model trees with a scorer failure are recovered through the original app session’s free Retry scoring/Re-score path. Afterwards:

```sh
"$FORGE_PYTHON" "$FORGE_OPS/campaign.py" recover --state "$FORGE_STATE"
```

`recover` makes metadata/billing reads and adopts the original result/launch identity; it cannot rebuild, publish or create a new benchmark session. It also retries owned stop/billing recovery for an already aborted unit. A resource-interrupted build cannot be reclassified as a completed capability result merely by re-scoring it. If source, task or SDK bytes changed, use the appropriate fresh qualification/freeze rather than blessing the tree with current identity.

After an owner-authorized ordinary resume, remove only the stop-between-units marker, then detach again. Do not remove `launch_blocked`, change the ledger, delete a unit directory or replace a completion receipt to force progress. After prospective task deepening and its verified new release/config:

```sh
"$FORGE_PYTHON" "$FORGE_OPS/campaign.py" refreeze --state "$FORGE_STATE" --config "$FORGE_INPUTS/next-frozen-config.json"
```

`refreeze` requires a different verified task fingerprint and no active original launch. It archives prior units, retains **every prior bill and original starting-credit ceiling**, and creates fresh isolated units without buying a call. Never run `init` again for this campaign.

Private state is written atomically with restrictive permissions. Keep `state.json`, `generations/*.native.json`, per-unit native request identity/envelopes, accounting and app verdicts, controller log and candidate trees together. Prompt bodies, credential snapshots and request auth headers are never copied into the narrow campaign receipts. Only the owner’s reviewed publication workflow can publish corrected results after live website readback.

Primary API documentation: [native generation bills](https://openrouter.ai/docs/api/api-reference/generations/get-request-&-usage-metadata-for-a-generation), [native prompt caching](https://openrouter.ai/docs/guides/best-practices/prompt-caching), [account credit readback](https://openrouter.ai/docs/api/api-reference/credits/get-credits), [current key limits](https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key).
