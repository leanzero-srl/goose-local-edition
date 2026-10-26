# Goose cross-device controller delivery

Updated 2026-10-26. Owner explicitly authorized unattended implementation until the app is ready for the marketplace, or a verified owner-only dependency prevents further meaningful progress. Execution stays on the original Mac. Earlier planning-only scope has been superseded by this authorization.

## Current operation

The active Codex goal owns continuation; do not finish merely because a phase or turn is complete. Long build/test jobs use tracked processes and persist receipts. This is feature delivery, not an unattended model benchmark: no background script is pretending to implement the backlog. Keep code committed, update this ledger at each measured increment, and continue after context compaction.

Current work: isolated iPhone/iPad mesh, lost-acknowledgement, replay and deliberate process-death recovery are proven. Worker enrollment and provider-attested roles are implemented (178 tests); the durable catalog passed its focused and existing regression tests; native UI passed 21 iPhone tests and 20 iPad tests including real Keychain operations. Actual owner execution is being detached from viewer lifetime and connected to catalog streams. No end-to-end production journey is claimed yet. Independent security review found a distributed-continuation lease regression and unauthenticated refresh-triggered provider cleanup; both must be corrected before rollout.

Working tree: `codex/link-controller-plan`, source base `e79a6f60ec`. Original checkout is untouched. Follow [architecture](link-remote-control-plan.md), [source evidence](link-remote-control-evidence.md) and [simulator receipts](link-ios-simulator-proof.md).

## Delivery gates

| Gate | State | Required evidence |
| --- | --- | --- |
| Architecture and Apple research; shared Claude/Codex lifecycle skill | COMPLETE | Source audit, independent reviews, canonical/snapshot match, existing Apple team identified. |
| Simulator toolchain and framework ABI | COMPLETE | Both virtual device families boot and run the measured C-handle positive/negative controls. |
| Embedded network transport | FIXTURE PROOF COMPLETE | Both device families passed HTTPS/WSS, wrong-token/no-proxy, lost acknowledgement, duplicate suppression and replay. Deliberate process death recovered one delivery. See link-ios-mesh-proof.md; production Link and real devices remain unproved. |
| Production contract and immutable identity | CONTRACT COMPLETE; implementation ACTIVE | Explicit owner/work IDs, migration/back-compat, DTO/endpoints, generations and cursor rules documented in link-controller-contract.md. |
| Controller enrollment/auth and exclusion from compute | IMPLEMENTED; review fixes/integration OPEN | Device-scoped credentials, receiver-side roles/capabilities, independent revocation, old-client behavior, every worker/job placement path excludes controllers. |
| Shared catalog and durable recovery | STORAGE PROVEN; adapters ACTIVE | Real chat/agent/benchmark/job adapters, incremental metadata feed, selected detail history, gap-free snapshot/replay, owner restart and stale cache tests. |
| Original-owner messaging and live control | OPEN | Owner run registry, viewer-independent lifetime, queued input/steer/stop/approval semantics, durable idempotency, stale run/generation conflicts and lost ACK proof. |
| Desktop cross-machine Sessions | OPEN | Real local and remote items, correct owner, transcript/controls, no accidental local activation, two-machine user-path proof. |
| Native universal controller UI | FOUNDATION PROVEN; network integration OPEN | Actual auth, Sessions/Nodes/Settings, native narrow/wide layouts, accessible real data, drafts/offline/error state, no compute/job worker. |
| Remote management and resources | OPEN | Real revision-pinned HF download/status/cancel, per-host provider setup, node enrollment, sampled resource age, secrets never read back. |
| Manual fleet sync | OPEN | Controller-local resumable task, partial/offline node outcomes, duplicate tap joins task, no inference or shell job spawned. |
| Integration, resilience and performance | OPEN | Actual Macs + both simulators, resource/network measurements, backpressure, duplicates, revocation, mixed versions, independent adversarial review and applicable repository gates. |
| Physical iPhone/iPad qualification | OPEN | Device install, cellular/IPv6-only, lock/background/relaunch, VoiceOver/keyboard, energy, real remote-host proof. Request owner intervention only when an actual needed device/unlock/pairing is unavailable. |
| App Store package and support readiness | OPEN | Signed archive/dSYMs, current Apple access/agreements, privacy/export/dependency review, truthful metadata/screenshots, reviewer account and host, support/privacy material, compatibility/hotfix runbook. Submission/publication is a separate release action. |

## Execution lanes and receipts

- `owner_run_service`: detached original-owner execution, durable command and approval handling, ACP observation, real disconnect tests; `work_control/` except backend/cursors and the ACP server seam.
- `controller_catalog_adapter`: read-only backend, device-bound signed cursors, fixed snapshots, safe user-visible messages and selected streams; backend/cursors, catalog read primitives and focused tests.
- `controller_security_review`: independent A1/A3 authorization, provider binding and Rust TLS/role review. Two confirmed findings require fixes before rollout.
- Main: build placement, integration, simulator evidence, source commits and release preparation. Next lanes are mobile actual mesh/TLS, host lifecycle, desktop Sessions and management.

Committed: worker A1 `2ada93953`, worker A3 `2bfcec031`, storage `a09533e1b`, native foundation `e41616ccf`, transport recovery proof `0d8f47b8e`, skill maintenance `8be3b9883`.

Storage gates on workmule: `laptop-20261026-165307-70db` build/focused 11 tests/Clippy passed; `laptop-20261026-170041-89e7` existing 35 session tests plus write-volume regression passed. The schema startup race was fixed at the exclusive WAL activation boundary and tested with eight actual processes.

A2 Link gate `laptop-20261026-171206-126d` failed inference fixtures after the process exhausted its inherited 256-descriptor limit. The exact primary failure is `Too many open files`; follow-on 502s are not accepted as a source regression or ignored without rerun. Earlier fixture lock inheritance was isolated by removing `HEAVY_RUN_HELD` only from that test's private lock helper. No production load guard was disabled. Source gate must run again with a suitable per-process descriptor budget and the review fixes.

C/read-adapter first compile snapshot: `laptop-20261026-172316-3760`, workmule isolated checkout `/Users/workmule/Projects/goose-controller-build`. Source overlay manifest `/tmp/goose-controller-rust-files.txt`; active writers may advance beyond this snapshot.

Native mesh physical-target framework build `laptop-20261026-171029-7f8d` passed, SHA256 `b69b4f22ac465eeac1f6e725b37dc357142c0598681b4f94354a4acddac56193`, platform iOS, minimum 18.1, SDK 27.1. It is an unsigned framework build, not a device installation or signed archive. Final native iPhone test job `laptop-20261026-165814-c9cc` passed 21 tests; iPad earlier job `laptop-20261026-165240-d985` passed 20.

No real Link secret is used by the isolated transport fixture. No production worker deployment or Apple submission has occurred. A fixture passing never completes a production journey gate.

## Stopping and recovery

Success means the named user journeys and release-readiness artifacts are proven, not merely compiling. A remaining physical-device, interactive login, legal agreement, seller declaration or final release approval is reported precisely only when required; complete independent work first. A failed technical experiment triggers investigation and an alternative, not a request for the owner to do engineering. Record failure evidence and do not silently change the architecture.

Current owner-only blocker: none established. No completion ETA is claimed before implementation throughput and verification requirements are measured. No model-work time cap or arbitrary inference budget has been introduced.
