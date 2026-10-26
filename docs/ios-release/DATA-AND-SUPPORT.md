# Data and support acceptance record

Internal working record, 2026-10-26. This is not a published privacy policy or a set of approved App Privacy answers.

## Actual data boundaries to verify

| Flow | Data | Boundary and remaining verification |
| --- | --- | --- |
| Email authentication | Email, OTP challenge, enrollment identifier and rate-limit metadata | Link authority and configured email delivery provider. Verify actual production logs, retention and deletion. OTP is never retained in native local state. |
| Device registry | Account/device identifiers, chosen name, platform, scopes, revision and revocation/refresh state | Strong authority registry; access/refresh credentials stay in Keychain on iOS. Verify backup and account deletion semantics. |
| Mesh enrollment | Public node keys, provider node identifiers, account association, IP/connectivity metadata and short-lived enrollment keys | Configured mesh authority; inspect pinned embedded SDK logging/telemetry and relay behavior. Cryptographic protection does not eliminate disclosure obligations. |
| Session viewing/control | Selected user-visible transcript, input answers, command IDs and delivery status | Native app to the original host over authenticated encrypted transport. Verify authority/relay logs contain no transcript, token or prompt bytes; user-owned host storage and configured AI providers have their own data paths. |
| Local recovery | Protected drafts, command journal, selected history/cache and refresh recovery ID | Native Application Support/Keychain; exclude from backups where required and clear the right account’s cache on access removal. Corruption is an error, not silently discarded state. |
| Model/provider management | Repository/revision, download status, host resources and provider configuration | Target host owns downloads and secrets. Never return stored provider credentials to controllers or diagnostics. |
| Support | App/build, OS, sanitized connection state, error class and opaque diagnostic reference | No automatic new analytics service is authorized. A user-initiated report must preview/redact contents; no transcripts, prompt text, tokens or host paths by default. |

Map these verified flows to Apple’s actual categories and purposes before publishing declarations. Email and identifiers already prevent an unsupported blanket “no data collected” assertion. Third-party AI disclosure/consent must identify where personal data is sent before sharing; consent is not inferred just from tapping a generic Send button. Source: [App Review Guidelines 5.1](https://developer.apple.com/app-store/review/guidelines/#privacy), checked 2026-10-26.

Account deletion must revoke controller credentials and remove the applicable authority/provider records under the published retention policy. Logout and device revocation are separate actions. Deleting a service account must not silently erase user-owned local session databases, model files or projects. An idempotent, authenticated deletion flow and full failure/retry proof are still required.

## Support procedure for this product

When a user reports a message did not arrive, obtain the existing command reference and query the owning Mac’s receipt. Never recommend sending another copy while delivery is unknown. Distinguish accepted, delivered, rejected and terminal failure; delivered does not mean the model obeyed.

When a node disappears, check authenticated discovery, host lease, provider membership, actual mesh liveness and client compatibility separately. An offline host is not an empty session history. Refresh/reconnect must keep known stale data labelled and must not cancel accepted owner work. An explicit authoritative revocation must stop new access.

Before each update, prove at least the oldest supported installed client against the additive backend, storage migration, refresh recovery, duplicate command behavior, manual partial sync and original-owner continuation. Record feature-level compatibility and exact build receipts. Do not rotate account-wide secrets or reset host identity to repair one controller.

For an incident, preserve minimal redacted reproduction, contain the affected capability using the existing authorized controls, reproduce on the affected release, independently review the fix and ship a newer verified build or compatible backend repair. App Store phased-release pause does not downgrade installed copies. No support SLA, customer email, recurring monitor or telemetry pipeline is created by this document.

Before release establish the reachable support/privacy URLs and the actual responsible support account, review-host maintenance, certificate/profile renewal procedure, compatibility policy and canonical release artifact/dSYM location. Keep these as verified records rather than invented contacts or promises. Generic platform procedures live in `local-edition/skills/ios-ipados-apps/references/operations.md`.
