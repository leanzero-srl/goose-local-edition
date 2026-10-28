# Goose Remote continuation

Updated 28 October 2026, 22:14 UTC (29 October local). This current summary supersedes historical statuses below in git history and older receipts.

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

**Current uncommitted work; do not package yet:**
- `release_reconcile` owns external agent/benchmark/shell sibling race. Log growth advances external revisions; oldlog_updated marker is ambiguous because it can overwrite semantic changes. Implement new internal pure-append marker only when complete normalized source metadata matches and known bytes strictly grow, then guarded stale revision bridging. Old ambiguous events remain strict. Preserve existing publiclog_updated frame/preview mapping. Fixtures only, no live Studio commands. Paths external catalog writer, sharedrevisionhelper, adapterscommands, backendmapping andtests.
- `desktop_steer_recovery` owns restored-pending UI gap: retry after reopening has emptycomposer; stale refusal removes pending payload while claiming draft retained. First prove via failing test, then retain/recover rejected text without overwriting existing different draft, cross-work/generation bleed, automatic resend or fake owner receipts. Hook/view/tests only.
- `merge_adversary` independently reviews both. `gabee_final_candidate` is idle with packaging preparation until final source/gates. Use followup_task to resume completed agents; send_message alone does not trigger them.
- Shared index is shared too: commit `--only` exact owned paths. A mixed commit was safely split; current commits above are scoped, no content lost.

## Build and package continuation

Warm Gabee source `/tmp/goose-controller-merged-c86a0b33/source`; target `/Users/mihaiperdum/Projects/goose-controller-test-build/target`; real UI dependencies and copied MCP resources already present. Do not create another target/dependency tree; local disk was37GiB free. Exact environment and remaining commands: `/tmp/goose-controller-steer-packaging-prep.md`.

All heavy work via node-dispatch. Last completed owner gate `laptop-20261029-000501-a9c5`; previousclient finalUI `laptop-20261029-000143-87ee`. Do not repeat passed client tests for unchanged files. New UI changes require relevant checks; new Rust external changes require affected tests and final workspaceClippy. Coordinate one owner of warm source before copying/building. Freeze and hash final tree, incrementally build CLI, then FULL normal desktop pnpm package. Its internal SDK/i18n steps are normal; no separate ACP schema regeneration without schema changes.

Full new bundle must be signed and validated. Private signing keys remain laptop-only; no Apple upload in sign-only job. Prepared `/tmp/goose-controller-corrected-sign-only.cjs` is unexecuted and needs review/pinning to actual new artifact. Narrow metadata validator permits only real signature attrs; roundtrip provenance exception applies only root/symlinks with bytes/modes/targets/otherattrs exact. Do not rerun old691checks or re-sign oldartifact.

Superseded signed ZIP retained as evidence at `/tmp/goose-controller-full-signed-c86a0b33/Goose-Swarm-Apple-submission.zip`, SHA3d124c48fb915f624a7bed4743082d3fbe490671e5be2e48c2a4909c17197c1b; validationreceiptc7723a86. Not submitted to Apple. Historical132 artifacts must never replace current apps.

After corrected package: freshidle checks, wholeapp swaps preserving currentdata, normal launch/reconnect, prove actual delivered activeSTEER (start waiter only AFTER real output begins, to exercise partial-only interval; initialsemantic refusal is correctly different). Use cloud only, persistcommand before send, check exactreceipt+actualUI. Do not rerun a model merely to fill waits.

## Main merge and fleet dependency

Feature branch `codex/link-controller-plan`; main NOT merged/pushed. origin/main freshly checked96e85f21ad; feature includes it via58e4bd3fc3, so fast-forward possible after final acceptance. Canonical checkout has unrelated dirty skill file; do not reset/check it out. No required branchchecks, but active main-push CI schedules6heavy selfhostedStudio jobs; runnerActionsidle does NOT mean modelidle, and nice10 is not memory protection. No existing guard/delay. Do not disable CI, stop runners or seize benchmark gaps. Publish testedfeature branch if appropriate; ordinary nonforce mainpush only when it cannot interrupt protected runs. Canary/docs workflows disabled manually; no public release requested by this merge.

Last22UTC node-probe: laptop4heavy slots occupied; workhorse Gauntlet35229 active with16%memoryfree; workmulehold98515 has95.7GB inference memory/activeCPU; Gabeeavailable. Re-probe AND inspect real campaign state before Studio upgrade or main CI. Neither Studio is upgraded by this task.

## Native and marketplace work remaining

Native signed IPA `/tmp/GooseRemote 2026-10-27 08-20-27/GooseRemote.ipa`, SHA dc2e77811e73b930d05336297afe692fe650b5bab46be3d0a49cdae6392ce0c9; archive `/tmp/goose-remote-release.xcarchive`; source04b224976/treeaae2514d6e20a36b67aea9bf1e823936c704078e; teamZZ8MTZ6NRZ. Xcode Organizer signing/export worked despite misleadingCLI“No Accounts”; do not ask login again on that basis.

iPadF7078B46-CCF8-4779-9070-0BAFE33ADFB7 retains login/controller/darktheme; never uninstall/reset. Existing-account mobile enrollment still refuses legacy usable hosts (intentional compute-credential boundary), so coretwoMacproof is separate from actualiPadjourney. iPhoneB7B9DE08-2F64-4573-8562-EC0F7C6FF2A9 hasfixturecoverage. No physicaldeviceverified. After genuine host readiness, proveiPad view/send/nudge/manualsync/resources/model/provider paths, controllerclosure leavesownerexecutionrunning. Then physicaldevice/lifecycle/accessibility, reviewaccount/host, privacy/export/support/marketing captures and organizationseller requirements. See docs/ios-release/{README,LAUNCH-PLAN,DATA-AND-SUPPORT,DEPLOYED-RETENTION-AUDIT}. No marketplace-ready claim yet.

## Operators and bookkeeping

Read-only/supported helpers `/tmp/goose-original-activity-probe.py PID`, `/tmp/goose-laptop-link-state.py PID`, `/tmp/goose-gabee-controller-catalog-probe.py PID`. Providerattestation `/tmp/goose-laptop-binding-readonly.py` runs via stdin on workhorse and prints only digestedkeys/allowedmetadata. Private snapshotfiles remain on their own Mac,0700/0600; never print/copy credentials or move them toAI SSD.

Heartbeat finish-goose-remote-testing active30min, quiet if unchanged. Update its prompt after meaningful state changes; disable only when testingcomplete or onlyowner-dependent work remains. GoalAPI stillpaused; tool cannot setactive. Owner was already informed once to pressResume; do notrepeat. Direct authority and heartbeat continue work.

CUA is the only UI automation surface. Root bindinggooseOriginal can be refreshed via getApp afterrestart; use freshAXindices. Screens show originalCloud controller test. No browser/CDP/osascript bypass. Intentional untracked ui/desktop/bundled-mcps symlink is never committed or used for packaging.
