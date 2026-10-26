---
date: 2026-10-26
status: source-audit
source_revision: e79a6f60eccecabc3a8dd8b3e4e2651568034572
---

# Evidence for Goose remote control

This map supports [the implementation plan](link-remote-control-plan.md). It distinguishes existing source from proposed behavior. The remote SHA was verified with `git ls-remote origin refs/heads/main`; a clean managed worktree was created from that revision. The original checkout and unrelated local work were left intact. The initial audit performed no Goose build, model run, device test, Apple login, submission or production change. The subsequent simulator-only feasibility proof is recorded separately below.

## Source anchors

Paths below are relative to the Goose repository at the pinned revision. Named functions/types are preferred over offsets that can move.

| Finding | Source to inspect |
| --- | --- |
| Historical iOS intent and read-only mirroring v1 | `local-edition/mlx/NOW.md`, “LEANZERO LINK — BUILDING THE DEEPER HALF”; `local-edition/mlx/LEDGER.md`, September 1 companion contract. Its email-derived token formula is superseded by `crates/leanzero-link/src/token.rs`. |
| Current node/session/stream, jobs, model and execution routes | `crates/leanzero-link/src/control.rs`, route construction around line 448; bearer/Origin middleware around 484. |
| Session identity and event wire model | `crates/leanzero-link/src/wire.rs`, `SessionSummary`, `SessionDelta`, `NodeState`, `NodeAllows`; `state.rs`, peer session mirror keyed by bare session ID. |
| Local IDs can collide across Macs | `crates/goose/src/session/session_manager.rs`, transactional DATE_counter ID generation around 1497. |
| Local IDs can also be reused after deletion | Same file, `create_session` derives suffix from MAX existing IDs; `delete_session` physically deletes rows. Public identity needs a never-reused work UUID. |
| Host identity is not yet immutable | `crates/goose/src/acp/server/link.rs`, `stable_node_id` uses current hostname plus a suffix absent before first connect; `crates/leanzero-link/src/manager.rs`, `node_hostname`. |
| Desktop local load also activates agent | `ui/desktop/src/acp/sessions.ts`; `crates/goose/src/acp/server/load_session.rs`, replay then activation; `ui/desktop/src/acp/chatSessionController.ts`. |
| Live run ownership and steering | `crates/goose/src/acp/server.rs`, `active_prompt_runs`, steer dispatch and current-run checks; `crates/goose/src/agents/agent.rs`, `steer` and input draining. |
| Last window can stop the host backend | `ui/desktop/src/gooseServeLeaseRegistry.ts`, `releaseWindow` and `releaseUnattached`; `main.ts` window close and quit integration. A hosting lease must be independent of windows. |
| Renderer-local queues/permissions | `ui/desktop/src/components/ChatInput.tsx`; `ui/desktop/src/acp/permissionRequests.ts`; `TurnElsewhereBar.tsx` found under `ui/desktop/src/components` reports the other-window limitation. |
| Idle remote continuation exists internally, not active steering | `crates/leanzero-link/src/state.rs`, `ExecuteRequest.session_id`; `crates/goose/src/acp/server/link_serve.rs`, executor; `link.rs`, remoteExecute constructs no session ID. |
| Live-only transcript tap and summary polling | `crates/goose/src/acp/server/link_serve.rs`, `tapped_reply`/delta bus; `link.rs`, `subscribe_local_deltas`. |
| Memory-only replay and too-old behavior | `crates/leanzero-link/src/pubsub.rs`, `PubSub` replay; `control.rs`, WebSocket stream; `state.rs`, mirror. Legacy `crates/goose-server/src/session_event_bus.rs` illustrates snapshot-before-replay handling but is not proof of desktop coverage. |
| Agent Work controls live in Electron | `ui/desktop/src/main.ts`, agent registry/list/start/stop/tick/pause/note/decision IPC handlers; files under the run's `.swarm/agent`. |
| Benchmark state and lifetime | `ui/desktop/src/benchSessions.ts`; `main.ts` benchmark exit/shutdown; `components/benchmark/BenchmarkView.tsx` integration with `SwarmRunPanel`. |
| Real Swarm notes and delivery receipt | `ui/desktop/src/main.ts`, note inbox writer; `components/swarm/SwarmRunPanel.tsx`, `NoteBox`; `crates/goose-cli/src/commands/swarm.rs`, inbox consumption and `user_notes_delivered`. |
| Concurrent notes can overwrite | `main.ts`, `swarm-add-note` writes `${Date.now()}.json` with normal truncating writes. `swarm.rs`, `note_epoch_ms` accepts timestamp prefixes; UUID suffixes preserve that parser contract. |
| New jobs/resource work absent in old laptop code | `crates/leanzero-link/src/jobs.rs`, `load.rs`, `queue.rs`; `manager.rs`, placement/admission; `ui/desktop/src/components/leanzero-swarm`, `RunJobDialog` and `MacLoadPanel`. |
| Jobs are not automatically durable prompts | `jobs.rs`, registry recovery distinguishes shell/process recovery from lost in-flight prompts; queue is memory-only. `read_log` reads full file before slicing, a performance seam to fix. |
| Existing HF/remote model operations | `crates/leanzero-link/src/state.rs`, MLX operation enum; `control.rs`, receiver permission gate; `crates/goose/src/acp/server/mlx_engine.rs` and remote adapters. |
| Download descriptor/durability gap | `crates/goose-sidecar/src/hf.rs`, `DownloadTracker` and tree/main listing; `crates/goose-sdk-types/src/custom_requests.rs`, `MlxEngineDownloadRequest`. |
| Worker is identity/enrollment, not a traffic relay | `leanzero-link/worker/src/router.ts`; `handlers/verify.ts`; `handlers/joinKey.ts`; `crates/leanzero-link/src/token.rs`. |
| Desktop visual system | `ui/desktop/src/components/lz`, `components/leanzero-swarm/studio.tsx`, `styles/main.css`, `theme/theme-tokens.ts`. |
| Old upstream mobile transport retired | `documentation/docs/experimental/remote-access/mobile-access.md`, `documentation/docs/experimental/goose-mobile.md`. |

Tracked-file searches in both the original HEAD and pinned remote found no Swift/Xcode native project scaffold; the known mobile documentation and repository tree were positive controls. This does not claim that no mobile prototype exists elsewhere on any machine.

## Apple account already in use

Goose's existing signing team is **ZZ8MTZ6NRZ**, identity `Developer ID Application: Mihai Perdum (ZZ8MTZ6NRZ)`. The current configuration uses `APPLE_ID`, `APPLE_ID_PASSWORD`, and `APPLE_TEAM_ID`; it does not establish an App Store Connect API key.

Current anchors: `ui/desktop/NOTARIZATION.md:80`, `ui/desktop/forge.config.ts:165`, `justfile:295`, and `ui/desktop/scripts/bootstrap-signing.sh:17`. The runbook names the workhorse as signing source and a dedicated Goose signing keychain. Secret files were not opened or sourced. Installed `Goose Swarm.app` metadata also reports this team; that is not a fresh signature trust/entitlement verification.

Actual earlier tool-output receipts, rather than unexecuted plans:

- Claude transcript `6fecca04-8d69-4ea2-b03d-d1da3d17162d.jsonl`, 2026-09-23T04:40:49Z, line 561: Gatekeeper accepted, Notarized Developer ID, origin Mihai Perdum with team ZZ8MTZ6NRZ. Same transcript line 5236 on September 24 reports the team identifier.
- Claude transcript `2b5af850-0385-431e-8595-058b9eb5c24b.jsonl`, 2026-10-01T21:55:30Z, line 1042, and October 4 line 17268: the same notarized/signing identity.

Both transcripts are under `/Users/mihaiperdum/.claude/projects/-Users-mihaiperdum-Projects-goose/`. These receipts establish earlier use; they do not verify current membership expiration, accepted agreements, ASC permissions, iOS profiles or seller organization type. The personal certificate name alone does not establish an individual versus organization enrollment.

Local toolchain readout: Xcode 27.1 (27A9275), iOS/iOS Simulator SDK 27.1, Swift 6.4. The restricted keychain enumeration reported zero valid identities; it does not disprove the historical account or establish that no certificates exist. No keychain unlocking/import was attempted.

Reuse the account/team. Inspect existing iOS assets before creating duplicates. [Apple membership coverage](https://developer.apple.com/programs/whats-included/), [certificate types](https://developer.apple.com/help/account/certificates/certificates-overview/), [App Store profiles](https://developer.apple.com/help/account/provisioning-profiles/create-an-app-store-provisioning-profile), and [credential reuse TN3147](https://developer.apple.com/documentation/technotes/tn3147-migrating-to-the-latest-notarization-tool) support the distinction between reusable identity/upload authentication and platform-specific signing.

## Primary transport and Apple sources

Read live on 26 October 2026. Dependency main-branch URLs need immutable revision pinning before implementation.

- Embedded userspace transport: [libtailscale](https://github.com/tailscale/libtailscale), [TailscaleKit](https://github.com/tailscale/libtailscale/blob/main/swift/README.md), [C API](https://github.com/tailscale/libtailscale/blob/main/tailscale.h), [Swift URLSession adapter](https://github.com/tailscale/libtailscale/blob/main/swift/TailscaleKit/URLSession%2BTailscale.swift). Current implementation returns configuration plus loopback information; an older README sample does not exactly match it. No physical-device integration was executed.
- Review/classification: [App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/), [complete review information](https://developer.apple.com/help/app-review/before-submitting-for-review/complete-review/). Native API control is an architectural distinction, not App Review preapproval.
- Build eligibility: [upcoming requirements](https://developer.apple.com/news/upcoming-requirements/), [submission guidance](https://developer.apple.com/app-store/submitting/). Check effective dates separately from announced future deadlines.
- Privacy/security: [App Privacy Details](https://developer.apple.com/app-store/app-privacy-details/), [account deletion](https://developer.apple.com/support/offering-account-deletion-in-your-app/), [privacy manifests](https://developer.apple.com/documentation/bundleresources/privacy-manifest-files), [SDK requirements](https://developer.apple.com/support/third-party-SDK-requirements/), [export compliance](https://developer.apple.com/help/app-store-connect/manage-app-information/overview-of-export-compliance/), [ATS](https://developer.apple.com/documentation/security/preventing-insecure-network-connections), [local-network privacy](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy).
- Platform UX/lifecycle: [layout](https://developer.apple.com/design/human-interface-guidelines/layout), [accessibility](https://developer.apple.com/design/human-interface-guidelines/accessibility), [background strategies](https://developer.apple.com/documentation/backgroundtasks/choosing-background-strategies-for-your-app).
- Operations: [TestFlight](https://developer.apple.com/testflight/), [phased updates](https://developer.apple.com/help/app-store-connect/update-your-app/release-a-version-update-in-phases/), [new version workflow](https://developer.apple.com/help/app-store-connect/update-your-app/create-a-new-version), [symbolication](https://developer.apple.com/documentation/xcode/adding-identifiable-symbol-names-to-a-crash-report), [MetricKit](https://developer.apple.com/documentation/metrickit).

## Verification status

Completed: source/history audit, current remote SHA check, official Apple/Tailscale documentation review, prior Apple signing receipts, independent architecture review and skill forward trials. Skill packaging validation checks structure and references; it is not an app test.

The independent source reviewer returned PASS after five confirmed defects were corrected: immutable host identity, backend lifetime after the last window closes, durable revision-pinned downloads, never-reused work identity after deletion, and collision-safe notes from concurrent controllers. The final command contract also checks store generation before acceptance/dispatch and handles revoked devices on open streams and pending commands. This is an architecture verdict, not runtime proof.

The iOS skill passed two independent forward scenarios: a remote AI controller preparing for review and a crashing phased update preparing a hotfix. Review-access/hardware evidence and removal/reinstatement rollout consequences were corrected; final verdict PASS. The standard skill validator passed. The Claude canonical source, Codex discovery link and five-file versioned snapshot were checked for identity, and local Markdown links resolve.

Follow-up simulator work: [build and runtime receipt](link-ios-simulator-proof.md). The diagnostic runs on an iPhone and iPad simulator; it proves framework loading and C-handle cleanup only. Link networking remains untested.

Pending for implementation: actual iOS mesh transport spike, real host-control behavior, performance measurements, physical-device accessibility/network/lifecycle proof, current Apple account/ASC/signing readiness, TestFlight and App Review. No green build, live-system success or approval is claimed for the proposed feature.
