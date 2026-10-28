# Goose Remote handover

Original handover27October2026; resumed by owner28October2026. **Work active again; not complete, not marketplace-ready.**

## Start here

The owner requested this handover and a pause on27October, then explicitly resumed this work and its goal on28October. Automatic continuation `finish-goose-remote-testing` is **ACTIVE** again. The operational resume record is the current source of progress. Running benchmarks remain protected by the idle-only upgrade approval.

Worktree: `/Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose`.
Branch: `codex/link-controller-plan`. The feature has **not been merged into main**. Do not assume a later checkout contains it. The3.0.134 source reconciliation is currently in progress; the intentionally untracked `ui/desktop/bundled-mcps` is a development symlink into the installed app, not source. Never package through it.

Read [the operational resume record](docs/plans/link-controller-resume.md) for exact artifacts, hashes, local helper paths, host identities and next actions. Read [the progress ledger](docs/plans/link-controller-progress.md) for evidence and historical corrections. Later dated receipts supersede earlier blocked states.

## Product the owner asked for

Desktop and iPhone/iPad must view sessions started on any linked Mac and send messages or nudges. Execution stays on its original Mac. Distinguish chats, Agent Work, shell/jobs and benchmarks from inference capacity. Mobile also manages remote Hugging Face downloads, nodes/providers and resources, supports multiple controller-only devices excluded from compute, and offers explicit manual fleet sync. Preserve desktop iconography, motion and themes with native mobile layouts. Prepare truthful SEO/App Store marketing and post-launch support.

## Historical delivered proof — 27 October

**Current28October change:** laptop and Gabee now run3.0.134 without the controller feature. A merge of origin/main96e85f21ad into this feature branch and a full new desktop build are in progress. Never downgrade them to the old3.0.132 candidate. Gabee returns controller/catalog method-not-found; ordinary Link remains connected. See the dated hold receipt and operational resume.

## Previously proved

- Native SwiftUI app and desktop/Link protocol implementation exist. Native package1.0.0/build1 is distribution-signed; desktop3.0.132 candidate is signed, Apple-notarized and independently verified. Signing is resolved.
- Original laptop and Gabee are upgraded and connected. A real command from Gabee's desktop controller facade reached an isolated laptop session, and the actual desktop displayed the expected assistant reply. This is two-Mac proof, not native-phone proof.
- Original laptop restart automatically reconnects without another code or retry. All73,324sessions and384,284messages retain complete-record hashes. Its real catalog has556entries (457chat/98benchmark/1agent_work); Gabee has55. The actual Linked Macs UI shows the original session and owner.
- Catalog scale correction7a0e558f5 passed focused query/scale checks,349Link tests (five existing ignored cases), scoped all-target Clippy and CLI build. Warm live sync measured639.5ms after restart; unchanged content preserved cache bytes/mtime.
- Shared iOS development/release/support skill exists for Claude and Codex: canonical `~/.claude/skills/ios-ipados-apps/`, discoverable through `~/.agents/skills/ios-ipados-apps/`, versioned snapshot `local-edition/skills/ios-ipados-apps/`.

## What remains

1. Finish reconciliation with3.0.134, required gates, and a newly built/signed desktop candidate. Then upgrade original workhorse and workmule **only when genuinely idle**, then verify enrollment, retained history/configuration, exact provider binding and restart. Read [host-upgrade procedure and receipts](docs/plans/link-controller-host-upgrade.md). Their current legacy hosts prevent iPad admission. Also align laptop/Gabee to the newly reconciled candidate when idle.
2. Prove the real iPad-to-Mac journeys: session history, send/nudge and actual response, command receipt/recovery, manual sync, resources, remote downloads/provider/node controls and controller exclusion. Preserve saved simulator authentication; never uninstall/reset to get a pass. Fixture/API success is not native UI proof.
3. Complete physical-device network/lifecycle/accessibility qualification and remaining review/privacy/export/support/marketing requirements in [release preparation](docs/ios-release/README.md) and [launch plan](docs/ios-release/LAUNCH-PLAN.md). No physical device was detected at last check.
4. Reconcile latest main, complete relevant final gates and independent review, then merge only with accurate final scope and existing release authority. Do not say the feature is merged or released until verified.
5. Organization conversion and final public release remain separate. No App Store record, upload, submission or publication has occurred. The signed IPA is a local export.

## Constraints and known traps

The owner approved original-profile upgrades WHEN IDLE, offline-verified-host admission implementation/deployment, the isolated cloud-backed integration test, the reviewed Link security fix and desktop notarization upload. Preserve these approvals; do not ask again for the same actions. No benchmark interruption, node deletion, credential copying, identity reset or weakened enrollment is authorized to force a pass.

At the last check,15:17 Bucharest/13:17UTC, workhorse Gauntlet benchmark61294 and inference94895 were still active; workmule was resource-busy. Absence of benchmark hold files or a temporary node-probe OK does not mean completion: the campaign launches subsequent work. Re-check current state rather than trusting these PIDs.

One original backend per profile. The completed isolated laptop app is closed because its controller TLS port41227 collides with the original app. `LEANZERO_LINK_CONTROL_PORT` changes a different listener. Keep `/tmp/gc-owner-a` and `/tmp/gc-ui-a` saved; do not restart it concurrently on41227.

Original laptop initially failed provider binding during migration. The existing expired-enrollment recovery succeeded through one normal UI retry after expiry, without a code change or authority bypass. Initial failure cause remains unproved. Initial-upgrade config hash changed without a captured original-content snapshot; full initial settings equivalence cannot be claimed. Config was identical through the monitored recovery and subsequent restart.

Apple Xcode Organizer exported successfully using existing cloud distribution signing even while CLI said No Accounts. Do not ask for Apple login based on that CLI error. TeamZZ8MTZ6NRZ; requested sellerLEANZERO S.R.L.; DUNS303218286; CUI51336260. The owner chose organization conversion before publication. Conversion was submitted and pending at last check; reverify later. See [conversion record](docs/ios-release/APPLE-ORGANIZATION-CONVERSION.md).

## Evidence and artifact map

- [Operational resume](docs/plans/link-controller-resume.md): exact final ZIP/IPA hashes and paths, process observations, helpers and protected local snapshots.
- [Desktop artifact receipts](docs/ios-release/desktop-controller-candidate/README.md) and [independent review](docs/ios-release/desktop-controller-candidate/review-7a0e558f5.md).
- [Native verification](ui/ios/VERIFICATION.md), [current native README](ui/ios/README.md), [protocol contract](docs/plans/link-controller-contract.md).
- [Worker deployment](docs/ios-release/LINK-WORKER-DEPLOYMENT.md), [retention audit](docs/ios-release/DEPLOYED-RETENTION-AUDIT.md), [data/support](docs/ios-release/DATA-AND-SUPPORT.md).

Temporary artifacts and private snapshots are local, not durable release storage. Validate existence/hash before reuse; never commit credentials or upload protected snapshots. Do not overwrite the single complete installed-app rollback. No broad cleanup or rebuild is needed merely to resume.
