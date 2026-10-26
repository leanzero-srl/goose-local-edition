# Goose cross-device controller delivery

Updated 2026-10-26. Owner explicitly authorized unattended implementation until the app is ready for the marketplace, or a verified owner-only dependency prevents further meaningful progress. Execution stays on the original Mac. Earlier planning-only scope has been superseded by this authorization.

## Current operation

The active Codex goal owns continuation; do not finish merely because a phase or turn is complete. Long build/test jobs use tracked processes and persist receipts. This is feature delivery, not an unattended model benchmark: no background script is pretending to implement the backlog. Keep code committed, update this ledger at each measured increment, and continue after context compaction.

Current work: the original-owner execution registry and read adapter are committed and tested. Worker replay cleanup and Rust distributed-continuation findings are corrected. Native production transport has passed 39 focused iPhone tests including the real embedded mesh with pinned HTTPS/WSS and replay regressions. Host lifecycle has passed a core build, five actual TLS/enrollment/lease tests and Link all-target Clippy. Independent review found missing permission/form presentation and a false-delivery permission race; those repairs are active. Desktop fleet viewing, non-chat adapters, management and final real-device/release qualification remain open. No installed end-to-end production journey is claimed.
Working tree: `codex/link-controller-plan`, source base `e79a6f60ec`. Original checkout is untouched. Follow [architecture](link-remote-control-plan.md), [source evidence](link-remote-control-evidence.md) and [simulator receipts](link-ios-simulator-proof.md).

## Delivery gates

| Gate | State | Required evidence |
| --- | --- | --- |
| Architecture and Apple research; shared Claude/Codex lifecycle skill | COMPLETE | Source audit, independent reviews, canonical/snapshot match, existing Apple team identified. |
| Simulator toolchain and framework ABI | COMPLETE | Both virtual device families boot and run the measured C-handle positive/negative controls. |
| Embedded network transport | FIXTURE PROOF COMPLETE | Both device families passed HTTPS/WSS, wrong-token/no-proxy, lost acknowledgement, duplicate suppression and replay. Deliberate process death recovered one delivery. See link-ios-mesh-proof.md; production Link and real devices remain unproved. |
| Production contract and immutable identity | CONTRACT COMPLETE; implementation ACTIVE | Explicit owner/work IDs, migration/back-compat, DTO/endpoints, generations and cursor rules documented in link-controller-contract.md. |
| Controller enrollment/auth and exclusion from compute | IMPLEMENTED; review fixes PROVEN; host integration ACTIVE | Device-scoped credentials, receiver-side roles/capabilities, independent revocation, old-client behavior, every worker/job placement path excludes controllers. |
| Shared catalog and durable recovery | STORAGE PROVEN; adapters ACTIVE | Real chat/agent/benchmark/job adapters, incremental metadata feed, selected detail history, gap-free snapshot/replay, owner restart and stale cache tests. |
| Original-owner messaging and live control | CORE PROVEN; input/reconnect repairs ACTIVE | Owner run registry, viewer-independent lifetime, queued input/steer/stop/approval semantics, durable idempotency, stale run/generation conflicts and lost ACK proof. |
| Desktop cross-machine Sessions | OPEN | Real local and remote items, correct owner, transcript/controls, no accidental local activation, two-machine user-path proof. |
| Native universal controller UI | MESH TRANSPORT PROVEN; final input/UI integration ACTIVE | Actual auth, Sessions/Nodes/Settings, native narrow/wide layouts, accessible real data, drafts/offline/error state, no compute/job worker. |
| Remote management and resources | OPEN | Real revision-pinned HF download/status/cancel, per-host provider setup, node enrollment, sampled resource age, secrets never read back. |
| Manual fleet sync | OPEN | Controller-local resumable task, partial/offline node outcomes, duplicate tap joins task, no inference or shell job spawned. |
| Integration, resilience and performance | OPEN | Actual Macs + both simulators, resource/network measurements, backpressure, duplicates, revocation, mixed versions, independent adversarial review and applicable repository gates. |
| Physical iPhone/iPad qualification | OPEN | Device install, cellular/IPv6-only, lock/background/relaunch, VoiceOver/keyboard, energy, real remote-host proof. Request owner intervention only when an actual needed device/unlock/pairing is unavailable. |
| App Store package and support readiness | OPEN | Signed archive/dSYMs, current Apple access/agreements, privacy/export/dependency review, truthful metadata/screenshots, reviewer account and host, support/privacy material, compatibility/hotfix runbook. Submission/publication is a separate release action. |

## Execution lanes and receipts

- `controller_host_lifecycle`: actual host registration, private host credentials, TLS listener composition, lease renewal/recovery/release and integration tests. No live deployment/enrollment performed.
- `ios_production_transport`: embedded mesh, pinned owner transport, recovery/receipt/history behavior, then typed form/permission presentation. iOS/iPadOS source only.
- `owner_input_repair`: original form schema and permission operation details, actual delivery result; subsequent idle needs-you/preflight/ACP replay gaps.
- Main: isolated build placement, source snapshot receipts, integration, durable progress and release records. Next independent lanes are desktop Sessions/lifetime and remote management/non-chat adapters.

Committed checkpoints: worker A1 `2ada93953`, worker A3 `2bfcec031`, storage `a09533e1b`, native foundation `e41616ccf`, transport recovery proof `0d8f47b8e`, worker replay fix `d4b0334789`, Rust controller/roles/continuation `14641105c2`, original-owner runtime/read adapter `1192cbaf47`, release/support draft `833fccb05d`, TLS qualification skill `df2ba57971`. Host worker tests `5fcfb2b320` currently depend on the active source cut; the next source commit must restore runnable HEAD without rewriting history.

### Measured gates

- Storage: workmule `laptop-20261026-165307-70db` build/focused 11 tests/Clippy; `170041-89e7` existing 35 session tests plus write-volume regression. Concurrent startup WAL activation tested with eight actual processes.
- Rust Link: `173726-ef77` full build, all-target tests **324 passed / 5 ignored**, all-target Clippy passed. Initial EMFILE fixture failure was traced to inherited `ulimit -n 256`, then rerun at 4096. No production guard was disabled.
- Core runtime/read adapter: `174406-403f` all 42 focused tests passed; final `174705-4fc0` all-target Goose Clippy passed after two boolean simplifications. Includes actual ACP connection-close/permission/old-run behavior, durable commands, catalog history, and legacy Serve executor.
- Host lifecycle: `175720-bc67` integrated build passed. `180649-99a7` build Link+Goose, five host lifecycle tests, Link all-target Clippy all passed. Actual TLS gateway fixture covers lost confirmation ACK, exact enrollment resume, renewal highwater/outage/recovery/revocation, private host secret/account boundary, and old-worker refusal. Snapshot hashes: `/tmp/goose-controller-host-test-snapshot.json`; final manager seam/full-suite gate still pending.
- Native: foundation iPhone 21 tests (`165814-c9cc`) and iPad 20 (`165240-d985`) passed. Current production-transport iPhone job `180325-bce1` **39 passed**, no skips/failures (author receipt; final source freeze/iPad gate pending). Includes actual mesh-pinned HTTPS/WSS, wrong-owner/wrong-pin, broken SOCKS/no-proxy, authorization close codes, pending-input cancellation and deleted/renamed history overlays.
- Device-target framework build `171029-7f8d` passed, SHA256 `b69b4f22ac465eeac1f6e725b37dc357142c0598681b4f94354a4acddac56193`, minimum iOS18.1. Unsigned framework is not a device installation or signed archive.

### Open findings and honest boundaries

Independent runtime review is `/tmp/goose-controller-runtime-native-review.md`. Typed form/permission details and actual callback delivery need the active repair. Native self-cancellation/history resurrection repairs have targeted tests; final reviewer verification remains pending. Original-owner idle ask-user continuation, detached recipe preflight and race-free local ACP partial replay remain unimplemented. Desktop last-window lifetime and cross-node viewing are also open.

The localhost URLSession TLS proof did not establish the mesh path: ATS rejected the private-IP custom trust before delegate processing. A broad insecure-HTTP ATS exception was rejected by automatic approval review and was not applied. The supported Network.framework TLS/SOCKS/WebSocket alternative now passes combined mesh tests with ATS unchanged. Saved-node reconnect without another auth key against real Headscale remains unproved; the fixture control server demands its fixture key again.

Release drafts live in `docs/ios-release/`. The existing Developer ID Application identity confirms team `ZZ8MTZ6NRZ`; it does not supply an iOS Distribution profile. No Apple account login, legal acceptance, upload, submission, production worker deployment or live credential enrollment has occurred. No physical iPhone/iPad is currently attached. Computer-use visual inspection encountered the locked Mac; code and simulator test work continues independently. Do not bypass that lock.

A fixture passing never completes a production journey gate. Performance remains open: selected-detail deltas exist, but catalog coalescing/latest-event projection and retained-journal bounds still require measurement and implementation.

## Stopping and recovery

Success means the named user journeys and release-readiness artifacts are proven, not merely compiling. A remaining physical-device, interactive login, legal agreement, seller declaration or final release approval is reported precisely only when required; complete independent work first. A failed technical experiment triggers investigation and an alternative, not a request for the owner to do engineering. Record failure evidence and do not silently change the architecture.

Current owner-only blocker: none established. No completion ETA is claimed before implementation throughput and verification requirements are measured. No model-work time cap or arbitrary inference budget has been introduced.
