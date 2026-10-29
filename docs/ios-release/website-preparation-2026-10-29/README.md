# INTERNAL: Goose Remote website patch preparation

Prepared 29 October 2026. Not deployed, not merged into the website and not a complete App Store privacy/support release. The applyable patch changes only the existing `/contact` page; privacy fragments are separately held in [PRIVACY-PENDING.md](PRIVACY-PENDING.md).

## Artifact and application

Website repository: `/Users/mihaiperdum/Projects/LeanZero-website`.
Exact fetched base: `c8cdb789d52fc35eefa06b852864cb1706c2a058`.
Patch: [goose-remote-support.patch](goose-remote-support.patch).
Receipt: [verification.json](verification.json).

Use an isolated website worktree at that base, with no upstream tracking master. Do not switch or stage the shared checkout (it is on `codex/seo-audit-improvements` with unrelated `newsroom/` and `origin/`). From the isolated worktree:

```sh
git apply --check /absolute/path/to/goose-remote-support.patch
git apply /absolute/path/to/goose-remote-support.patch
```

The patch adds `#goose-remote` using existing `Section`, `SectionHeader` and page classes. It imports no components or dependencies, adds no route/form/analytics, and preserves the existing form and SEO/canonical. The existing `/contact` store URL remains valid; an anchor can be used later after rendered link acceptance.

## Verified now

Extracted the exact base contact file into an isolated temporary Git directory; forward `git apply --check`, actual apply, exact expected-output comparison and reverse apply check all passed. TypeScript 5.9.3 parses the patched TSX with zero diagnostics. This is syntax/application evidence, not a TypeScript project build, rendered design approval or deployed-page test. No heavy build ran alongside the backend gate. Source and patch hashes are recorded in verification.json.

## Claim-to-primary-source challenge

Reviewed Goose source at `4c5283aa39e75542c8fdaf59e2ab8e8bf1caa9b6` plus the already committed physical receipts.

- Online/compatible selected Mac: owner-scoped transport and compatibility behavior documented by [physical iPhone proof](../physical-iphone-proof-2026-10-29.json) and the [data/support inventory](../DATA-AND-SUPPORT.md). The wording says saved activity “may” be out of date, not that every disconnect loses data.
- Check delivery and Accepted versus Delivered: [SessionDetailView.swift](../../../ui/ios/GooseRemote/Features/SessionDetailView.swift) renders the actual Check delivery control and owner receipt; [ControllerStore.swift](../../../ui/ios/GooseRemote/App/ControllerStore.swift) checks original command/work identity, clears a matching draft only on Delivered, and does not turn delivery into model completion. [Actual cold-send receipt](../physical-iphone-cold-send-43a-primary-2026-10-29.json) distinguishes delivery and reply. Refuted the temptation to promise a visible command ID: the public copy asks for a reference only “if you have one”.
- Remove local access and deletion labels: [SettingsView.swift](../../../ui/ios/GooseRemote/Features/SettingsView.swift) and [AccountDeletionSection.swift](../../../ui/ios/GooseRemote/Features/AccountDeletionSection.swift) show the exact controls, emailed-code flow, pending cleanup and status button. [Account deletion service](../../../leanzero-link/worker/src/controller/account-deletion.ts) revokes access before cleanup. It cannot erase Mac-local projects/models/session databases. No complete-erasure claim or deletion deadline was added.
- Mailbox: the native Settings support link and pending-deletion support link already target office@leanzero.net. This proves a configured contact, not staffing, guaranteed response or legal adequacy. Requested diagnostic context and exclusions are reporting instructions, not a claim that the app uploads those data.
- AI/tool sharing: [physical pre-send disclosure and decline](../physical-sharing-disclosure-2026-10-29.md) explicitly reported OpenRouter and developer tool, both destination not reported. Copy says configured services/tools “can” receive content and onward recipients may be missing. It does not claim “no extensions”, all-local content or exhaustive destinations.

## Publication holds and remaining checks

The existing contact page promises 24-hour email replies and 8-hour critical acknowledgement elsewhere; the existing SLA expressly covers Marketplace apps and consulting. The new section adds no SLA promise, but that surrounding scope ambiguity remains a publication hold until mobile applicability/coverage is resolved. Do not interpret this patch as silently expanding or cancelling existing commitments.

Privacy remains incomplete for the precise retained-record/operator questions in PRIVACY-PENDING.md and DATA-AND-SUPPORT.md. Do not publish the pending fragments alone as an adequate mobile privacy policy. Known legal identity/contact are already established, not missing owner inputs.

Before website release, run the relevant typecheck/lint/build and production-build rendering under the website skill/resource guards; verify the anchor clears the fixed header, content/reveal behavior, phone/desktop light/dark, existing form/link behavior and metadata. Obtain independent factual/public-copy review. No support message, policy publication, deployment or App Store submission is authorized or performed by this preparation.
