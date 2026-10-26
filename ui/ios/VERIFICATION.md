# Native controller verification

2026-10-26; Xcode 27.1 (27A9275), iOS Simulator 27.0; deployment floor 18.1. These are simulator and isolated network-fixture receipts, not physical-device or production-worker qualification.

The foundation's 21 tests passed in `laptop-20261026-165814-c9cc`. This includes real Keychain read/write/update/delete after the unsigned startup failed, exact owner/run/choice command fencing, protected draft/receipt persistence, rotating refresh lost-ACK recovery, pagination, and canonical wire parsing. Ad-hoc simulator signing and explicit app/Keychain entitlements are essential; mocks did not reveal the original Keychain failure.

Transport checkpoint iPhone job `laptop-20261026-180325-bce1` completed `TEST SUCCEEDED`: **39 tests, 0 failures, no skips**. This linked the actual pinned TailscaleKit framework and exercised isolated native mesh HTTPS/WSS, mandatory owner/SPKI verification, renewed 90-day leaf on the same key, wrong key/owner/expired/future/client-only rejection, redirects refused, close 4401/4403/4413, persisted node reconstruction with stable PublicKey, broken SOCKS refusal and direct no-proxy refusal against the mesh owner. The independent OpenSSL SPKI digest is `4379a9ee08e0d4f37685ce498c090125088f046413298a5d2cce99b60a127616`.

Replay tests cover noncontiguous global revisions, UTF8 byte offsets, duplicate frames, partial-to-commit replacement, partial discard, owner/generation mismatch, durable cursor/selected-only cache reopening, catalog entry revision below history watermark, pending-input stream reload without self-cancellation, and live deletion/rename surviving arrival of an older history page. Late history is ordered before a newer live answer.

Final iPad job `laptop-20261026-180818-e61b` passed 39 tests, 0 failures, no skips. Final iPhone job `laptop-20261026-181338-2035` passed **40 tests, 0 failures, no skips**, adding exact owner page-order preservation when timestamps tie. It includes selection fencing against stale input reload and removal of unused URLSession certificate code. Independent review is `/tmp/goose-controller-runtime-native-review.md`: R2 and R3 native defects were repaired with regressions; R1 typed input and R4 original-callback receipt delivery remain in the next integration. No native TLS/framing bypass was confirmed.

## Failed attempts and what changed

`laptop-20261026-173834-7e37`: a TLS fixture with an unrealistic 21-year certificate failed Apple SSL policy; replaced with bounded leaf validity, then matched Rust's 90-day policy. Localhost TLS checks subsequently passed.

`laptop-20261026-174653-0a5d`: localhost pin checks passed but combined mesh failed because ATS rejected the private self-signed IP before URLSession custom trust. Automatic approval review rejected a proposed CIDR ATS exception; no exception was applied. Network.framework now handles the specialized owner TLS/WS path, with the same mandatory verifier; account-authority URLSession/ATS is unchanged.

`laptop-20261026-175258-9c8b`: real mesh HTTPS/WSS passed; reconstruction failed because testcontrol requires the auth key on every registration, even a known node. The fixture's reconstruction supplies its disposable key and asserts unchanged node identity. Production drops the one-use key after confirm, so actual Headscale keyless resume remains unproved.

`laptop-20261026-175733-bf85`: 37 tests, 1 failure. Network.framework bypassed SOCKS for localhost, disproving that proposed negative control. Production now rejects non-mesh endpoints before connecting. Actual mesh tests independently show broken SOCKS and direct no-proxy requests cannot fetch the owner nonce. Do not claim a global Network.framework no-bypass property.

## Evidence boundaries

The fixture starts local testcontrol/DERP and a tsnet owner with disposable credentials; it does not contact production Link or read existing credentials. Per-run evidence is under `/tmp/goose-remote-derived/mesh-run-<UUID>/requests.jsonl`; request records contain no tokens. Test private keys are harmless committed fixtures. Product startup cannot load test resources.

Parent visual proof covered signed disconnected startup, white primary-button text, persistent labels, visibly disabled email action, and iPad split view. Connected screen journeys, typed permission/form prompts, installed-owner execution, physical iPhone/iPad, real worker enrollment/revoke/recovery, IPv6/cellular/background lifecycle, accessibility and 18.1 runtime remain unproved. The Mac was locked on the latest UI attempt; it was not bypassed.

## Active typed-input checkpoint

`laptop-20261026-182116-83e6` passed **47 tests, 0 failures, no skips** on iPhone with the testcontrol mesh fixture. Seven new tests cover typed permission arguments with exact signed/unsigned 64-bit IDs, required fields/enums, explicit boolean selection, Unicode string constraints, exact integer bounds, unsupported schema refusal, missing/unknown payload refusal, and command-builder validation of JSON values. Root independently reviewed the complete presentation/view and JSONValue changes; no defect was confirmed in the supported flat-schema path. This is not connected-owner UI proof. Rust contract baseline is `8b1e52fd8`; future idle question/preflight DTO changes are not included.

`laptop-20261026-182200-b3d0` ran the same 47 tests against isolated real Headscale v0.29.3: **1 failure**. Initial native node pinned HTTP/WSS succeeded. The reconstructed node used nil authKey, retained identical PublicKey and reached Running, but its first HTTPS SOCKS dial to `100.64.0.1:10002` timed out after 5 seconds (POSIX22). Thus saved identity resume without enrollment credentials is proven; resumed owner traffic remains failed. Root owns the fixture and investigation. Other 46 tests passed. No real account credentials or production Headscale were used.

## Account deletion and durable recovery — 2026-10-26

The native settings flow now uses the worker's account-deletion request-code, confirm and status endpoints. It saves the request ID before requesting email, saves the purpose-only token before confirmation, and records the uncertain confirmation state before dispatch. A wrong code retains normal access; lost acknowledgement recovers by status. Accepted deletion clears the known account credentials, mesh enrollment, provider outbox, revoke-UUID records and protected app caches while preserving deletion recovery until server and local cleanup are complete. Unrelated Keychain records are not deleted. Reopening and foreground recovery route to Settings. Pending deletion exposes its request ID and retry; optional contact does not replace in-app completion.

iPhone job `laptop-20261026-202155-b32f` passed 23 focused tests. iPad job `laptop-20261026-202641-0ea9` passed 34 tests, including core auth, nine management tests and ten deletion tests. After removing the backend's obsolete support-required classifier, the final iPad job `laptop-20261026-202953-4b1b` built the final Swift UI and passed all nine remaining deletion tests. These cover exact paths/body/header roles, interrupted request reuse, wrong OTP, lost-confirmation recovery after OTP expiry, pending logout, local cleanup failure, mismatched receipt refusal, durable-save-before-send, and real isolated-Keychain cleanup preserving recovery and unrelated records. The backend lane confirmed the wire and retained categories. Root independently reviewed the state machine and requested the pending retry/request-ID presentation.

The SwiftUI build is proven; an interactive deletion screen, real deployed worker deletion and physical device journey are not yet proven. No deployment, App Store upload or account deletion was performed. Automatic approval review rejected broad service-wide Keychain cleanup before execution; the explicit credential allowlist and revoke-UUID-only implementation above was accepted and tested.
