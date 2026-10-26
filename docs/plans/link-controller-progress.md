# Goose cross-device controller delivery

Updated 2026-10-26. Owner explicitly authorized unattended implementation until the app is ready for the marketplace, or a verified owner-only dependency prevents further meaningful progress. Execution stays on the original Mac. Earlier planning-only scope has been superseded by this authorization.

## Current operation

The active Codex goal owns continuation; do not finish merely because a phase or turn is complete. Long build/test jobs use tracked processes and persist receipts. This is feature delivery, not an unattended model benchmark: no background script is pretending to implement the backlog. Keep code committed, update this ledger at each measured increment, and continue after context compaction.

Current work: prove HTTPS and WebSocket through embedded userspace mesh on the virtual iPhone/iPad. Parallel work maps the production device-auth/catalog/control contract. The previously committed handle probe proves only framework loading and cleanup.

Working tree: `codex/link-controller-plan`, source base `e79a6f60ec`. Original checkout is untouched. Follow [architecture](link-remote-control-plan.md), [source evidence](link-remote-control-evidence.md) and [simulator receipts](link-ios-simulator-proof.md).

## Delivery gates

| Gate | State | Required evidence |
| --- | --- | --- |
| Architecture and Apple research; shared Claude/Codex lifecycle skill | COMPLETE | Source audit, independent reviews, canonical/snapshot match, existing Apple team identified. |
| Simulator toolchain and framework ABI | COMPLETE | Both virtual device families boot and run the measured C-handle positive/negative controls. |
| Embedded network transport | ACTIVE | HTTPS/WSS nonce round-trip through mesh, authenticated source evidence, wrong-token/no-proxy controls, reconnect, two controllers. Fixture proof must remain distinguished from actual Link. |
| Production contract and immutable identity | OPEN, design lane active | Explicit owner/work IDs, migration/back-compat, DTO/endpoints, generations and cursor rules, named file ownership. |
| Controller enrollment/auth and exclusion from compute | OPEN | Device-scoped credentials, receiver-side roles/capabilities, independent revocation, old-client behavior, every worker/job placement path excludes controllers. |
| Shared catalog and durable recovery | OPEN | Real chat/agent/benchmark/job adapters, incremental metadata feed, selected detail history, gap-free snapshot/replay, owner restart and stale cache tests. |
| Original-owner messaging and live control | OPEN | Owner run registry, viewer-independent lifetime, queued input/steer/stop/approval semantics, durable idempotency, stale run/generation conflicts and lost ACK proof. |
| Desktop cross-machine Sessions | OPEN | Real local and remote items, correct owner, transcript/controls, no accidental local activation, two-machine user-path proof. |
| Native universal controller UI | OPEN | Actual auth, Sessions/Nodes/Settings, native narrow/wide layouts, accessible real data, drafts/offline/error state, no compute/job worker. |
| Remote management and resources | OPEN | Real revision-pinned HF download/status/cancel, per-host provider setup, node enrollment, sampled resource age, secrets never read back. |
| Manual fleet sync | OPEN | Controller-local resumable task, partial/offline node outcomes, duplicate tap joins task, no inference or shell job spawned. |
| Integration, resilience and performance | OPEN | Actual Macs + both simulators, resource/network measurements, backpressure, duplicates, revocation, mixed versions, independent adversarial review and applicable repository gates. |
| Physical iPhone/iPad qualification | OPEN | Device install, cellular/IPv6-only, lock/background/relaunch, VoiceOver/keyboard, energy, real remote-host proof. Request owner intervention only when an actual needed device/unlock/pairing is unavailable. |
| App Store package and support readiness | OPEN | Signed archive/dSYMs, current Apple access/agreements, privacy/export/dependency review, truthful metadata/screenshots, reviewer account and host, support/privacy material, compatibility/hotfix runbook. Submission/publication is a separate release action. |

## Execution lanes

- `ios_mesh_fixture`: isolated Go control/DERP/owner fixture; owns `scripts/ios-controller-probe/mesh-fixture/` only.
- `ios_mesh_client`: simulator HTTPS/WSS diagnostic; owns `MeshProbe.swift`, `build-mesh.sh`, `README-mesh.md` in the probe directory only.
- `controller_contract`: production source mapping/contract; owns `link-controller-contract.md` only before assigning code cuts.
- Main: orchestration, job placement, simulator execution, actual evidence, integration and commits.

No real Link secret is used by the isolated transport fixture. No paid model request, production provider alteration or migration of existing sessions is needed for this gate. A fixture passing never changes the production feature's OPEN status.

## Stopping and recovery

Success means the named user journeys and release-readiness artifacts are proven, not merely compiling. A remaining physical-device, interactive login, legal agreement, seller declaration or final release approval is reported precisely only when required; complete independent work first. A failed technical experiment triggers investigation and an alternative, not a request for the owner to do engineering. Record failure evidence and do not silently change the architecture.

Current owner-only blocker: none established. No completion ETA is claimed before implementation throughput and verification requirements are measured. No model-work time cap or arbitrary inference budget has been introduced.
