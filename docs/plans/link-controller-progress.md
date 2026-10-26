# Goose cross-device controller delivery

Updated 26 October 2026. The owner authorized unattended delivery until marketplace readiness or an actual owner-only dependency. Original-machine execution is fixed: other Macs, iPhones and iPads observe and control that owner. The earlier architecture-only scope was superseded.

## Current work

Branch `codex/link-controller-plan`; original checkout untouched; feature NOT merged into main. Worktree `/Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose`. No release, installed production journey or App Store readiness claimed.

User direction: run lean and parallel, avoid speculative redesign and repeated full suites. Use focused checks for changed paths, one integrated final verification, and additional testing only for actual failures. The earlier 2–4 day / 1–2 week estimates were unsupported and withdrawn. High is the recommended normal implementation reasoning setting; this task did not change the user's setting.

Implemented and tested earlier: scoped controller identity/exclusion from compute, owner registration and pinned TLS, durable owner/work/run identity and command receipts, actual chat continuation and inputs, native mesh transport, desktop linked sessions and application-owned backend lifetime. Native management now uses the owner's existing model downloader/provider configuration with Keychain retry storage; nine focused management/recovery tests passed.

Current coordinated Rust gate covers management, external catalog adapters and detached recipe preflight. Desktop benchmark bridge is implemented and under focused verification. The final external-log path is being corrected to open a bounded latest tail and update on real log growth; older history remains explicit. Actual two-Mac + native UI integration, signing/release requirements and final gate remain open.

Architecture/source references: [plan](link-remote-control-plan.md), [evidence](link-remote-control-evidence.md), [wire contract](link-controller-contract.md), [real Headscale proof](link-ios-headscale-proof.md).

## Measured checkpoints

- Storage: `165307-70db` isolated build, 11 focused tests and Clippy; `170041-89e7` 35 existing session tests plus write-volume regression; concurrent WAL startup exercised with eight actual processes.
- Link/auth/runtime: full Link gate `181804-aa0e` passed 331 tests/5 explicit existing ignores, nine input tests and Goose/Link all-target Clippy. Worker host authority passed 194 tests. Owner runtime/read path and lease/revocation/account boundaries committed.
- Native saved-mesh repair `11aad5e39`: real isolated Headscale 0.29.3; iPad `185814-40c2` and iPhone `185933-5b28` passed 51/51 tests each. Includes saved node identity without another enrollment key, wrong TLS pin/owner, lost acknowledgement, replay and 30 immediate authorization close frames. The first SOCKS connect EINVAL repair only retries before HTTP bytes are sent. No broad ATS exception, production transport upgrade or post-send automatic retry.
- Desktop UI `5d56134762`: 560 files / 5,224 tests passed, three existing skips; typecheck/lint/i18n passed. This is UI test proof, not installed two-Mac proof. Facade worker tests: 198 passed, tsc and Wrangler dry run passed.
- Idle owner questions `f3e433afe6`, native support `1b1dcb3bbe`: exact original choices and nil-current-run fence. Focused nine native input tests passed `191210-345b`.
- ACP replay `21f4f46ffb`: original pre-run boundary and exact notification replay; actual rewrite/order/disconnect tests passed `191459-6f72`.
- Desktop facade review P1 fixed: failed initial outbox save could be bypassed on exact retry. Candidate state now persists before every send; actual TLS test passed `191459-6f72` (zero owner requests on failed saves, same command recovery after repair). Account reassignment suspicion was refuted; account boundary stays retained.
- Native management `193515-57c7`: eight tests passed. Store-generation 409→receipt404 recovery added and passed `193837-5e64`. Independent review `/tmp/goose-management-review.md`: progress target mismatch, invalid-payload outbox wedge, and missing-receipt recovery fixed. Explicit clear never claims delivery/rejection and never undoes owner work.
- Current combined Rust attempts `193214-7c79`, `193727-d46a` found concrete new-code/test compiler errors; corrected without changing runtime behavior to satisfy tests. Subsequent gate runs remain tracked in node-dispatch. Do not call them passed until their final output is read.

## Open delivery work

1. Complete focused Rust management/catalog/preflight checks and affected Clippy; commit coherent shared source (a prior shared-file staging error placed management declarations before its implementations, so the working-tree checkpoint must close that dependency).
2. Prove external latest-tail growth and older pagination in desktop/native, plus benchmark bridge/actual cancellation and inbox consumption. Metadata fanout is coalesced; full transcript text only travels for a selected work item. Chat durable fragment journal writes are unchanged; do not claim reduced chat disk writes.
3. Generate the changed ACP SDK once, run the final integrated desktop/native/owner journey and applicable gate, and independently review the new control boundary. Keep original Macs/production identities safe during isolated testing.
4. Complete signed iOS archive, account/seller/App ID/provisioning, dependency/cryptography/privacy/account-deletion assessment, real device network/lifecycle/accessibility, final icon/screenshots, review account/host and support URLs. A simulated device is useful but does not prove a physical device's cellular/background/energy behavior.
5. Merge only after the feature is complete and tested. App Store upload/submission/public launch are distinct external actions, not inferred from a build.

Remote HF management deliberately reuses current latest/main download behavior. No downloader/revision-pin/restart-resume redesign. An owner restart can lose its in-memory transfer tracker: display missing/unknown explicitly, never resumed/completed. Add-Mac setup reuses actual desktop Link enrollment; mobile controllers never create fake hosts or enter the inference pool.

Manual sync uses a controller-local task with per-owner results and stale/offline states. Verify it in the final UI journey; no inference/shell job should be launched by refresh.

## Release and marketing

[Release preparation](../ios-release/README.md), [data/support inventory](../ios-release/DATA-AND-SUPPORT.md) and [first App Store launch plan](../ios-release/LAUNCH-PLAN.md) are internal drafts. Launch plan covers truthful App Store copy, branded/descriptive search terms, website SEO, real screenshots/video, publisher attribution and post-launch support. Name availability and seller identity are unverified; no search-volume or market-first claim.

Existing Apple Developer ID Application identity identifies team `ZZ8MTZ6NRZ`; membership can be reused, Developer ID itself cannot sign the iOS App Store build. Physical-target embedded framework compiled, but is not a signed app archive. No Apple login/legal acceptance/upload/submission, production worker deployment or live host enrollment has happened here. No physical phone/tablet was attached at last inspection; computer-use inspection encountered the locked Mac. Do not bypass authentication. Complete independent engineering before an owner-only handoff.

## Continuation

The active goal owns continuation through compaction. Keep small tested commits, exact receipts and actionable handoffs; do not stop at a phase boundary. Prior failed experiments remain in git history and linked proof records. A fixture passing does not complete the production-journey gate. Ask only for an actual necessary owner-held login, unlock, device, legal declaration or final external release action.
