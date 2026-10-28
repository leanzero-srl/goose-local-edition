# Goose Remote continuation

Updated 28 October 2026, 22:32 UTC (29 October local). This current summary supersedes historical statuses below in git history and older receipts.

## Authority and non-negotiable constraints

- Owner authorized unattended completion, preserving original sessions/settings/identity, upgrading idle original Macs, and merging with main **if integration works**. Latest priority is the two MacBooks. Do not interrupt either Studio or seize a between-run gap.
- Latest explicit constraint: use cloud models/cloud nodes for inference tests. Keep local models unloaded on both MacBooks; they are busy with unrelated work.
- Execution remains on the session's original Mac; controllers only view/control it. Controllers must not enter inference pools.
- Apple seller must become LEANZERO S.R.L. before publication. Conversion was last confirmed pending; browser login later expired. No App Store upload/submission occurred.
- New 3.0.134 Apple notarization upload was rejected by automatic approval review because earlier explicit approval named 3.0.132. A question for ZIP3d124c48 was subsequently presented, then owner was told to HOLD it because the steering fix changes the payload. Do not upload that superseded ZIP or apply its answer to another artifact. Finish corrected signed ZIP before new exact-artifact approval.

## Installed MacBooks: working viewing and continuation, active-steer fix not installed

Both original apps are complete signed 3.0.134 controller candidate, CLI SHA88cdf89cde29c490cccbd0eb4e999b367f2d301aa77eb5120704c509bd30ae0c, runtime source58e4bd3fc3/treec86a0b33. Normal LaunchServices startup succeeded without OS warning or bypass. Both clean restarts automatically reconnect after supported recovery.

- Laptop original app/backend156/190 at last check; Gabee backend73537. Re-read PIDs before actions.
- Laptop73324 preexisting sessions/384284 messages retained exact full hashes; Gabee16/37 retained exact full hashes. Gabee config byte-identical. Laptop changed only ordering of adjacent openai/openrouter YAML map entries; every key/value and all other lines unchanged, including after cloud test selection.
- Whole previous3.0.134 apps retained locally at `~/.local/share/goose-upgrade-rollback-3.0.134-20261028/Goose Swarm.app`. Historical132 rollback is separate and untouched. Never downgrade or patch a signed embedded component.
- Saved-login recovery: previous legacy release changed only provider pre-auth key ID (laptop307→321, Gabee319→324), while node/key/user/IP matched. Supported authenticated hosts/release then ordinary Connect re-registered the same owner and required new exact provider confirmation/activation/lease. No node deletion, binding edits, credential transfer or identity reset. Laptop actual UI Retry was used. Provider nodes33/45, machine keys/IPs/owners unchanged; normal reauth rotated public node keys. Full confirmed/activated/provider bindings match. Durable procedure in `local-edition/skills/goose-feature-dev/references/host-enrollment-recovery.md` and skill master.
- Both catalogs sync fresh: laptop557 entries after new test, Gabee55. Inactive/legacy owners remain unavailable; do not call all of them Studios without attribution.

## Actual cloud test and the two repairs

Disposable ORIGINAL laptop session `20261028_1`, title `Cloud controller test`, public WorkRef chat `c787fd2d-0909-4f49-9e85-bd48a684f020`, owner83b81dd1-ec2c-485a-89ec-2d9d43a93d90. Created in actual UI with explicit node `node:deepseek-v4-1-flash-openrouter` (swarm provider, named cloud route). Baseline actual reply: “Cloud controller test ready.”

Gabee controller sent one persisted commandb793cd28-af41-4bba-9f29-d79b30156716. Accepted21:46:00, delivered21:46:12; actual laptop UI showed “Gabee remote continuation confirmed.” No tool calls/files/local inference. Gabee has no existing authenticated cloud provider: its OpenAI endpoint is localhost, Claude CLI auth is loggedOut. Do not use those defaults or copy credentials for reverse testing.

Active test then sent a cloud-only120-line list request. Gabee attempted steer6d1f0c2d-d1f0-4006-bd40-309cbd2455d2 at observedrev73471/run2e95d091-140c-4b49-aefb-94fbe784bf69. Typed owner409stale_revision at21:50:51.866082, receipt404, no steer delivered, no retry. List completed; test idle.

**Primary trace matters:** rev73471→73476 included message_upsert/session_changed as prompt persisted, so original request MUST still reject. Then135partial rows73477..73611 changed no catalog metadata exceptrevision/event_kind. Exact new SQL against real read-only DB gives73471→73477/73611=false,73477→73478/73611=true. Receipt `/tmp/goose-active-steer-real-trace.json`.

Committed fixes (NOT yet packaged/installed):
- `501394485d`: desktop clears only definitive pinned-owner typed409stale_revision unaccepted outbox entry, persists removal before reporting, refreshes snapshot/preserves current draft/requires explicit new action. Unknown/accepted IDs immutable. Gates7Rustclient+9targetUI+changedcratebuild/Clippy+lint+fullUI5347pass3skip. Fullsuite mirror issue was ignored package-time src/swarm-bench; final clean-layout suite passed and mirror restored.
- `46eb0bcd2d`: chat Steer/Stop shared revision helper keeps exact fast path, otherwise requires anchors and only same-run partial events with all control metadata unchanged. Used at admission and dispatch. Original73471 still rejects; refreshed73477 tolerates subsequent chunks. Gates17owner incl7new+11catalog+workspacealltargetsClippy pass (joba9c5). Independentreview+real historicalSQL proof pass.

## Final corrected package built; one signing job queued

- `70a40d48f5`: reopened pending-command recovery, confirmed failing regression then21targetUItests+lint PASS; independent review PASS.
- `bf5966b19fdf4835b8feadcbe95b2951c7f3498b`, tree `67c316961e92ade0f6f10a156beeb0972f34993d`: external agent/benchmark/shell correction. Reserved `verified_log_append` is emitted only when complete normalized source metadata matches and known bytes strictly grow; shared admission/dispatch proof requires anchored same-work/same-run pure output. Historical ambiguouslog_updated/semanticchanges/truncation/unknown stay strict. Wire preview/frame remainlog_updated. Preventive sibling fix; no live benchmark commands.
- Final gates:20work_control PASS job9dcc; shell fixture private import corrected, then8external+11catalog+workspacealltargetsClippy PASS job `laptop-20261029-002450-f27e`. No unchanged20test rerun. Independent source/SQL/adversarial review PASS.
- Full CLI+normal Electronpackage job `laptop-20261029-002647-2562` PASS.4332tracked build inputs matched before/after; no tracked generated changes. Corrected unsigned CLI `5885afff41ac2d273f8212688000ebbc733b09ebcd22e34c89d10b1d3f4039d5`.
- Corrected unsigned ZIP exists on laptop AND Gabee at `/tmp/goose-controller-steer-bf5966b19fdf/Goose-Swarm-unsigned.zip`, SHA `28b13239fd482303019fb7046a5edf9821b2aeb820f2141dd4fd21d5d1af2b93`,457686364bytes. Buildreceipt adjacent SHA `0af22991ea79d15b287b9079d581f98f6df1f0ddbcc8c03945a443a6eea7f70a`; durable copy `docs/ios-release/desktop-controller-candidate/build-receipt-bf5966b19fdf.json`.
- **ONE local sign-only job queued** `laptop-20261029-003113-df5a`, unified execsession57445, waitcap1800s, waiting behind laptop4/4heavy slots and low memory. Do not duplicate/re-sign. Node-run will place normally when8GBheadroom+slot exists. Check queue/job and `/tmp/goose-controller-corrected-signed-bf5966b19fdf/receipt.json` before actions. If queue expires, record that explicitly; do not mistake it for signing failure or permission rejection.
- Exact reviewed sign command: `NODE_PATH=/Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose/ui/node_modules node /tmp/goose-controller-corrected-sign-only.cjs /tmp/goose-controller-steer-bf5966b19fdf/build-receipt.json /tmp/goose-controller-steer-bf5966b19fdf/Goose-Swarm-unsigned.zip /tmp/goose-controller-corrected-signed-bf5966b19fdf`. NO Apple upload/notary/install. Normal heavy job, privatekey stayslaptop. Signscript+metadata validator independent review PASS. Freshstage only; don't reset a partial stage blindly.
- Old3d124c48ZIP remains superseded and not uploaded. No new Apple question until corrected signed artifact verified.

## After signing: corrected whole-app upgrades and cloud proof

Prepared `/tmp/goose-install-corrected-controller.py` takes RECEIPT SOURCE_APP SIGNED_FINGERPRINT APP_PID BACKEND_PID OWNER_ID. It pins finalsource/tree/newCLI, complete incoming fingerprint to receipt, strict Apple/team signature, copiesbeforequit, fresh owncatalog+sessionactivity twice, waitsowneddescendants, swapswholeapp, retains oldcandidate separately and normalLaunchServices. Independent review found/fixed two gaps: entirebundle must matchreceipt (notCLIalone), and any nonnullcurrent_run_id isbusy (idle canmean livewaitingagent/queuedshell). FinalreviewPASS. Fingerprinter `/tmp/goose-bundle-fingerprint.py` SHA b2eaa9d4023b8bf97db598f928270851bdaa00377bff9367b58401b206892849; idlehelpers `/tmp/goose-original-owner-idle-probe.py` + `/tmp/goose-original-activity-probe.py` must exist on target. No profilewrites; original134rollback untouched. Currentcandidate rollback planned under `~/.local/share/goose-controller-rollbacks/88cdf89cde29c490/Goose Swarm.app`. RecheckPIDs.

Final cloud test helpers prepared, independently reviewed, UNEXECUTED: `/tmp/goose-gabee-cloud-steer-final-once.py` and `/tmp/goose-gabee-cloud-steer-final-result.py`; procedure `/tmp/goose-controller-final-cloud-steer-procedure.md`. First observe actual output in originalUI with explicit cloudroute, then one fresh anchored snapshot/one durableUUID/one command; no implicitretry. Preserve new private finalreceipt; result onlyreads exactreceipt/history. Delivered+unique message+assistant response+actualUI required. Earlier refusal must remainfailed.

**Mac UI locked** at22:25UTC. CUA cannot unlock; ownerasked once asynchronously to unlock. Continue non-UI work; do not claim finalUIproof or bypasslock. No response yet. Currentprocesses still laptop156/190, Gabee73528/73537; laptopfreshcatalog557entries/noactive also after runidcheck. Correctedcandidate is NOT installed yet.

WarmGabee source `/tmp/goose-controller-merged-c86a0b33/source`, target `/Users/mihaiperdum/Projects/goose-controller-test-build/target`; maintain until completed. Disk29GiB beforefinalbuild;37 was priorobservation, notfloor.15GiB guarded buildfloor. No reason to repeatpassedtests or duplicatebuilds. Sharedindex: exact `--only` commits; intentionaluntracked bundled-mcps symlink nevercommit.

## Main merge and fleet dependency

Feature branch `codex/link-controller-plan`; main NOT merged/pushed. origin/main freshly checked96e85f21ad; feature includes it via58e4bd3fc3, so fast-forward possible after final acceptance. Canonical checkout has unrelated dirty skill file; do not reset/check it out. No required branchchecks, but active main-push CI schedules6heavy selfhostedStudio jobs; runnerActionsidle does NOT mean modelidle, and nice10 is not memory protection. No existing guard/delay. Do not disable CI, stop runners or seize benchmark gaps. Publish testedfeature branch if appropriate; ordinary nonforce mainpush only when it cannot interrupt protected runs. Canary/docs workflows disabled manually; no public release requested by this merge.

Last22:25UTC node-probe: laptop3/4heavy slots,33.3GB above memory floor; workhorse Gauntlet35229 active with17%memoryfree; workmulehold98515 has95.8GB inference memory/activeCPU; Gabee1/4heavy runningourfinalgates. Re-probe AND inspect real campaign state before Studio upgrade or main CI. Neither Studio is upgraded by this task.

## Native and marketplace work remaining

Native signed IPA `/tmp/GooseRemote 2026-10-27 08-20-27/GooseRemote.ipa`, SHA dc2e77811e73b930d05336297afe692fe650b5bab46be3d0a49cdae6392ce0c9; archive `/tmp/goose-remote-release.xcarchive`; source04b224976/treeaae2514d6e20a36b67aea9bf1e823936c704078e; teamZZ8MTZ6NRZ. Xcode Organizer signing/export worked despite misleadingCLI“No Accounts”; do not ask login again on that basis.

iPadF7078B46-CCF8-4779-9070-0BAFE33ADFB7 retains login/controller/darktheme; never uninstall/reset. Existing-account mobile enrollment still refuses usable legacy Workmule provider node36 (intentional compute-credential boundary; nodes33/45 MacBooks have exact confirmed/activated bindings). Read-only currentpolicy remains account-isolated. Node36 online, untagged, nonexpiring, no matching enrollment; merelydisconnecting is insufficient because usable ignoresonline. Do not revoke/edit it or bypasspolicy, so coretwoMacproof is separate from actualiPadjourney. iPhoneB7B9DE08-2F64-4573-8562-EC0F7C6FF2A9 hasfixturecoverage. No physicaldeviceverified. After genuine host readiness, proveiPad view/send/nudge/manualsync/resources/model/provider paths, controllerclosure leavesownerexecutionrunning. Then physicaldevice/lifecycle/accessibility, reviewaccount/host, privacy/export/support/marketing captures and organizationseller requirements. See docs/ios-release/{README,LAUNCH-PLAN,DATA-AND-SUPPORT,DEPLOYED-RETENTION-AUDIT}. No marketplace-ready claim yet.

## Operators and bookkeeping

Read-only/supported helpers `/tmp/goose-original-activity-probe.py PID`, `/tmp/goose-laptop-link-state.py PID`, `/tmp/goose-gabee-controller-catalog-probe.py PID`. Providerattestation `/tmp/goose-laptop-binding-readonly.py` runs via stdin on workhorse and prints only digestedkeys/allowedmetadata. Private snapshotfiles remain on their own Mac,0700/0600; never print/copy credentials or move them toAI SSD.

Heartbeat finish-goose-remote-testing active30min, quiet if unchanged. Update its prompt after meaningful state changes; disable only when testingcomplete or onlyowner-dependent work remains. GoalAPI stillpaused; tool cannot setactive. Owner was already informed once to pressResume; do notrepeat. Direct authority and heartbeat continue work.

CUA is the only UI automation surface. Root bindinggooseOriginal can be refreshed via getApp afterrestart; use freshAXindices. Screens show originalCloud controller test. No browser/CDP/osascript bypass. Intentional untracked ui/desktop/bundled-mcps symlink is never committed or used for packaging.
