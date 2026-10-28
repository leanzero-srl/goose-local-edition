# Controller reconciliation with desktop 3.0.134

28 October 2026. Validation/build in progress; not an installed or released candidate.

## Motivation and provenance

Installed laptop and Gabee moved to3.0.134 while the controller task was paused. Gabee actual ACP returns -32601 for controller/catalog. Its ordinary Link remains connected. The old signed3.0.132 candidate must not downgrade either host.

Feature parent1a7ba6df83 merges origin/main96e85f21ad. Initial reconciled tree8972b0a5423517fc3c9621816b92faf71568cf76; corrected build treec86a0b3313848a80995cd52e04bb4b06f780f6c9. Fifteen locale conflicts were resolved by semantic three-way key comparison, retaining57controller keys per locale and main translations. PeerRegistry reconciliation combines controller admission with main online seeding, poll verdict and early node publication. Main stack sizing, independent custom-request tasks, config memoization, SSH environment cleanup and UI fixes are imported. A full SDK/main/preload/renderer rebuild is required. The Link worker subtree was already identical between branches.

## Independent review and correction

Independent reviewer merge_adversary traced admission, compute consumers, liveness, leaving notices and ACP dispatch. One confirmed preexisting P2: a mesh peer excluded before host authorization could remain absent indefinitely after authorization, because authority renewal installed only the roles while the peer-sync loop reconciled only changed mesh tuples. Online peers can retain last_seen=None, so normal polling is not a reliable rescue.

Correction: PreparedHost activation and scheduled renewal receive the manager's configured shared peer control port. After successful lease/admission/router preparation, renewal installs the authoritative snapshot and reconciles its observed mesh peers through the existing admission-protected registry. No credential, enrollment or permission bypass is introduced.

Trace: unknown mesh host → excluded row → unchanged mesh tuple → later authoritative Host role → scheduled renewal → reconciliation → visible eligible host at the configured peer URL. Would change outcome: YES on the first successful renewal after host authorization. Controllers remain excluded before row/task creation.

Production-entry regression `authority_renewal_admits_an_existing_mesh_host_without_a_mesh_change` exercises actual activate→scheduled renewal, unchanged full MeshStatus, later Host role, a nondefault shared port, controller exclusion and preserved credentials. Combined peer_liveness tests cover seeded online hosts and exclusion/revocation. Reviewer rechecked corrected treec86a and returned PASS; runtime tests remain separate.

Rejected suspicions: online seeding bypassing admission; early folds overriding leaving notices; liveness reversing a settled Offline poll verdict; a concrete harmful change from combined per-handler/outer boxing and custom-request spawning. Existing compute/outbound checks and notice-generation/poll-verdict guards were traced.

## Validation job

Node-dispatch `laptop-20261028-201650-d684` on Gabee; stage `/tmp/goose-controller-merged-c86a0b33`. Script `/tmp/goose-controller-merged-gabee-job.sh`. Immutable source archive plus checked binary delta, warm target reused without editing the warm checkout. Initial formatting check passed and Rust compilation started. Exact exit, test totals and package hashes must be appended after completion. No signing, notarization, app install or runtime launch occurs in this job.

The merge commit waits for Clippy. A started job is not passing proof. Keep original histories/settings and current installed apps untouched until the new candidate passes and each target is genuinely idle.

## Gate progress — 28 October, 18:36 UTC

Initial job completed formatting,39affected Goose integration tests, the catalog query-plan test and358Link tests (5existing ignored; separate ACP helper ignored). Actual scheduled-authority-renewal regression passed. Workspace Clippy then stopped in vendored cpp-httplib: Homebrew OpenSSL4.1.0 returns const X509_NAME*, incompatible with the dependency source. No product source was modified for this environment issue.

Job `laptop-20261028-203350-4b90` resumes the same tree at Clippy with installed OpenSSL3.6.2 selected explicitly and its failed native CMake cache reconfigured. Current log confirms OpenSSL3 include/libs and continued compilation. CLI/schema/desktop/package completion remains pending. No passed tests rerun, no installed app changed.

## Completed build — 28 October, 18:52 UTC

Resume job4b90 exited0. Workspace all-target Clippy, CLI build, full-feature schema generation, SDK build, desktop typecheck/ESLint/i18n and complete desktop package passed. All567UI test files passed:5343tests,3skipped. Generated schema/SDK source and i18n message source remained identical. Exact machine-readable receipt: `build-receipt-3.0.134.json`.

Unsigned local archive `/tmp/goose-controller-merged-c86a0b33-unsigned.zip`, SHA256 `ce361f66af6158fd8b9c89adee5c9d103562546ec94f5efa63a08dcdaafb1895`. Embedded CLI SHA256 `5ba611eee51ee68160c4161f7236ab3e173f0fda0e844b55e8fca9bd64bc7586`. Source changes after the tested tree are documentation only. Signing/notarization and actual host/native journeys remain separate; no installed app changed.

## Independent unsigned-artifact review

Reviewer merge_adversary independently streamed the ZIP and embedded CLI, verified both SHA256 values and main Info.plist3.0.134/net.leanzero.goose-swarm, checked gate script hashes and read raw remote logs/exit files. Source commit58e4bd3fc3 differs from tested treec86a only in documentation. Aggregate initial Rust results398passed/6ignored equal39core+1query-plan+358Link; scheduled-renewal test explicitly passed. Resume exit0, generated source unchanged,567UI files/5343tests/3skips, and complete packaging were confirmed. Verdict PASS for unsigned artifact/provenance; signing, notarization and actual user paths still require their own proof.
