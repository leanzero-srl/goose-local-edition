# Goose cross-device controller delivery

Updated 26 October 2026. The owner authorized unattended delivery until marketplace readiness or an actual owner-only dependency. Original-machine execution is fixed: other Macs, iPhones and iPads observe and control that owner. The earlier architecture-only scope was superseded.

## Current work

Branch `codex/link-controller-plan`; original checkout untouched; feature NOT merged into main. Worktree `/Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose`. No release, installed production journey or App Store readiness claimed.

User direction: run lean and parallel, avoid speculative redesign and repeated full suites. Use focused checks for changed paths, one integrated final verification, and additional testing only for actual failures. The earlier 2–4 day / 1–2 week estimates were unsupported and withdrawn. High is the recommended normal implementation reasoning setting; this task did not change the user's setting.

Implemented and tested earlier: scoped controller identity/exclusion from compute, owner registration and pinned TLS, durable owner/work/run identity and command receipts, actual chat continuation and inputs, native mesh transport, desktop linked sessions and application-owned backend lifetime. Native management now uses the owner's existing model downloader/provider configuration with Keychain retry storage; nine focused management/recovery tests passed.

Coherent Rust source and generated SDK are committed; external latest-tail/history, benchmark bridge, note consumption and Agent Work controls passed their focused checks. The final desktop gate passed 562 files / 5,257 tests (three existing skips), typecheck/lint/i18n. Native iPhone gate passed 78 tests, including real isolated Headscale traffic and theme assets. Actual desktop launch then found a default Tokio worker stack overflow in custom-request dispatch; its repair and runtime proof are in progress. Two-Mac/native production integration, signing and physical-device qualification remain open.

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
- Coherent owner source `c6472dcd76`: final core/external checks `201536-7bdd`, CLI `--lib remote_agent_` four tests and all-target Goose/CLI/Link Clippy `201937-df00` passed. The earlier CLI `--bin` filter ran zero tests and is not counted. Schema + actual goose CLI build `202206-169e` passed; source hashes matched the transferred executable.
- Benchmark bridge `3ec860483`: 36 desktop tests passed; actual delivery receipt follows owner consumption. A raced archived-log mapping is rechecked before returning bytes/cursor. External old history explicitly pauses live replacement.
- Generated SDK `9ed484e23` and final desktop gate `203244-9488`: 562 files, 5,257 passed, three existing skips; typecheck, lint and 16-locale i18n checks passed.
- Native deletion `07d548901`: purpose-token recovery, lost acknowledgement and local cleanup proof. Known credential keys only are cleared; unrelated Keychain entries are preserved.
- Embedded mesh privacy `44a89aa52`: actual Go constructor disables diagnostic collection/upload before node creation. Local negative/control probe measured control one upload / three buffer writes, protected zero / zero, including an old buffered log. Simulator/device framework receipts and manifests are required at embedding.
- Native `204321-fd5f`: 78/78 passed, including five appearance cases and actual isolated Headscale traffic. Actual visual review subsequently found invisible navigation titles in both themes; removing forced navigation-bar backgrounds restored them. Focused visual correction is tracked separately, not concealed by the green unit gate.
- Worker deletion `895702b45`: owner explicitly approved the uncertain-provider-operation security patch. Final 219/219 tests, typecheck and Wrangler dry-run passed. Independent actual helper probes confirmed unknown 503/malformed success outcomes remain pending across restart, while recognized success/rejection settles. No deployment, live deletion or email in this lane.
- Actual isolated desktop launch: 20:51, backend pid 71471 aborted with a Tokio worker stack overflow in `dispatch_custom_request`; screen showed Unable to connect to Goose server. Evidence `/tmp/gc-ui-a/logs/goose-serve-stderr.log` and macOS crash `goose-2026-10-26-205119.ips`. Existing ACP fixtures use larger 8 MiB worker stacks. No runtime pass claimed until structural repair is exercised in the real app.

## Open delivery work

1. Repair and re-prove the actual desktop dispatch stack overflow on the ordinary runtime. Preserve the default thread stack; measure the dispatcher/handler future boundary and exercise the actual UI.
2. Finish native desktop iconography/theme/motion visual proof, signed archive attempt and privacy answers. Theme unit/asset proof does not replace visible navigation checks.
3. Deploy the coherent worker only with its tested source, exact rollback and retained account/KV/Headscale state. Then prove actual two-owner and native account/session/control integration. Never manufacture a successful sign-in or owner delivery.
4. Complete iOS account/seller/App ID/provisioning, actual cryptography/export declaration, physical-device network/lifecycle/accessibility, review account/host and public support/privacy material. A simulator does not prove cellular/background/energy behavior.
5. Merge only after the feature is complete and tested. App Store upload/submission/public launch remain distinct outcomes.

Remote HF management deliberately reuses current latest/main download behavior. No downloader/revision-pin/restart-resume redesign. An owner restart can lose its in-memory transfer tracker: display missing/unknown explicitly, never resumed/completed. Add-Mac setup reuses actual desktop Link enrollment; mobile controllers never create fake hosts or enter the inference pool.

Manual sync uses a controller-local task with per-owner results and stale/offline states. Verify it in the final UI journey; no inference/shell job should be launched by refresh.

## Release and marketing

[Release preparation](../ios-release/README.md), [data/support inventory](../ios-release/DATA-AND-SUPPORT.md) and [first App Store launch plan](../ios-release/LAUNCH-PLAN.md) are internal drafts. Launch plan covers truthful App Store copy, branded/descriptive search terms, website SEO, real screenshots/video, publisher attribution and post-launch support. Name availability and seller identity are unverified; no search-volume or market-first claim.

Existing Apple Developer ID Application identity identifies team `ZZ8MTZ6NRZ`; membership can be reused, Developer ID itself cannot sign the iOS App Store build. Physical-target embedded framework compiled, but is not a signed app archive. No Apple login/legal acceptance/upload/submission, production worker deployment or live host enrollment has happened here. No physical phone/tablet was attached at last inspection. Xcode 27 simulator UI is available through DeviceHub.app; do not infer it is missing from the old Simulator.app path. Complete independent engineering before an owner-only handoff.

## Continuation

The active goal owns continuation through compaction. Keep small tested commits, exact receipts and actionable handoffs; do not stop at a phase boundary. Prior failed experiments remain in git history and linked proof records. A fixture passing does not complete the production-journey gate. Ask only for an actual necessary owner-held login, unlock, device, legal declaration or final external release action.
