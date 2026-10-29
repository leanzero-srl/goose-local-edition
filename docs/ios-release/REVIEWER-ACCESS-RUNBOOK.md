# Goose Remote: dedicated App Review access runbook

Prepared29October2026. Internal operational draft. No review account/mailbox/host has been provisioned by this document, no secret is stored here and no review access has been submitted. Run the setup only after the listed resources/access are authorized. It uses the existing email-code sign-in and local Mac enrollment; it does not propose a fixed OTP, authentication exception or controller-created host credential.

## Inputs and release gates

Known: intended seller LEANZERO S.R.L.; support/privacy address office@leanzero.net; native app identity net.leanzero.goose-remote; email sign-in/deletion through Resend; original Mac enrollment requires local acceptance. Existing actual phone/controller and owner receipts show the product path works, but none establishes an isolated reviewer environment.

Record these before setup, keeping credentials in the approved secret store rather than this file:

| Input | Current state | Completion evidence |
| --- | --- | --- |
| Dedicated review email/inbox and approved way for Apple to receive fresh codes | Not identified/provisioned | Account owner authorizes dedicated access; rehearsal retrieves a fresh native sign-in code without personal inbox access |
| Dedicated online Mac/profile with no personal sessions/files/credentials | Not allocated | Host owner authorizes resource; isolated catalog and provider credentials verified |
| Final accepted native + compatible desktop version and distribution build | New compute-node feature still in progress | Exact build/commit receipts and actual required paths; do not borrow baseline proof for new actions |
| Review contact and availability | office@ is published; staffing/coverage unconfirmed | Named responsible operator confirms monitoring during review; no invented service promise |
| Third-party inference account/model and permitted fixture use | Not selected for review | Authorized dedicated provider credential and applicable processing/spend decision |
| Organization seller and first App Store record | Conversion requested, unresolved | Actual Apple organization/seller confirmation; no duplicate conversion request |

A short-lived OTP entered into review notes days in advance will not remain valid. The worker checks challenge expiration (`leanzero-link/worker/src/controller/service.ts`). Choose an authorized, reviewer-usable route to the **dedicated** mailbox and rehearse its login/MFA dependencies. Supplying a mailbox credential is a separate access decision, not permission inferred from this runbook. If that cannot be arranged, obtain an explicit access arrangement with App Review before submission; do not promise staff will deliver codes at an unspecified moment or silently bypass sign-in. An approved full demo mode would be a separate implementation/Apple decision, not an existing capability.

## Operator setup and acceptance

1. Record the chosen Mac/profile, account email, responsible operator and exact desktop/native versions in a private operational record. Do not repurpose either Studio's active work or expose the existing personal MacBook catalogs. Confirm the separate profile contains only review data before enabling remote access.
2. Install the accepted Goose Swarm desktop build on the dedicated Mac. Through its normal UI open Providers → LeanZero MLX → My Macs, sign in with the dedicated review account and select Connect to mesh. Enable Manage models and sessions on This Mac for the operations being reviewed. This follows the existing Add a Mac instructions; the phone does not install or enroll a remote Mac for it.
3. Configure only the authorized review provider/model through supported desktop controls. Create a small real synthetic conversation, such as an explanation of a fictional packing checklist, containing no customer names, private paths or production material. Use normal app actions; do not inject database rows or prelabel a fabricated receipt as delivered. If live inference is included, record that authorization and its actual result.
4. On a clean dedicated physical controller, sign in through the ordinary fresh-code flow and Sync registered Macs. Verify that its complete catalog shows only the dedicated review host/work. Revoke access and correct account isolation before proceeding if any personal host appears. Capture metadata/boolean evidence rather than secrets.
5. Rehearse from a fresh app launch: open the synthetic session, read the latest reply, scroll into older text, use Jump, Refresh while reading, then send one clearly marked synthetic request after inspecting the actual sharing disclosure. Observe Accepted/Delivered separately from the model's reply; verify exactly one user message and receipt. A lost acknowledgement means Check delivery for the original reference, never another Send to manufacture success.
6. Exercise only capabilities actually advertised by the final host. Provider/download baseline proof does not substitute for the pending compute-node feature. When that feature is accepted, use an unused uniquely named definition, verify persisted fields through supported reads and remove only that captured definition with no cascade flags. No model load/default/strategy/pool change is part of node-definition creation. Do not invent an enrollment-invitation demonstration.
7. Verify a short background/reopen and the release's outstanding physical network/lifecycle/accessibility cases separately. Preserve the original personal iPad simulator; it is not this disposable reviewer fixture. Device capability/profile registration must be checked for each physical iPad rather than inferred from the phone profile.
8. Keep the dedicated Mac awake, logged in, network reachable and its backend accessible for the review window using an authorized operational arrangement. Record an actual reconnect check from outside the host's local network when supported; an open desktop window alone is not availability proof. Do not alter shared fleet power/network policy as a side effect.
9. Before submission, another operator follows only the proposed reviewer instructions from a clean controller. They must acquire a fresh code, discover only the intended host, complete the demonstration and understand cloud-provider disclosure without assistance from Mihai's personal account. Record failures and fix the instructions/environment before calling access ready.

Account deletion must remain available in app. Use a separate disposable account for the planned populated deletion/recovery proof, since deleting the shared review account would revoke its host/controller access. If Apple deletes its dedicated review account, recover through the documented normal enrollment/setup flow with authorized operators; never undo deletion invisibly or restore a revoked credential. Exact operational recovery and retained-record decisions remain in ACCOUNT-DELETION-RUNBOOK.md and DATA-AND-SUPPORT.md.

## Review notes worksheet

Populate only after the rehearsal; this worksheet is not ready for pasting with placeholders.

- Explain that the phone/tablet is a native controller, not screen mirroring or an inference node. The selected Mac coordinates its configured AI provider, which may be cloud-based; model downloads use its configured storage.
- Give final app/desktop versions, minimum supported compatibility, tested physical devices/OS versions, concept/audience, free-companion business model and required internet/online Mac.
- Supply the isolated sign-in email and approved fresh-code/inbox instructions through App Store Connect's private review fields. Keep working credentials out of repository files, screenshots, public URLs and video frames.
- Name the dedicated Mac and synthetic conversation, then provide numbered UI steps for sign-in → sync → session → sharing disclosure → one Send → reply/receipt. Add actual steps for other advertised operations after acceptance.
- Explain any unavailable capability honestly and describe expected offline behavior. Do not label unsupported routes as reviewer mistakes or promise a fixed review start time.
- Supply the reachable review contact and the physical-hardware demonstration attachment. Record any Apple-agreed alternative authentication arrangement and its scope.

## Physical video and store captures

Apple's [complete-review guidance](https://developer.apple.com/help/app-review/before-submitting-for-review/complete-review/) asks for working demo credentials, authentication-code information and a video of a physical Apple device pairing/interacting with required hardware. A screen recording alone does not meet that stated hardware-video format. Its [review guidelines](https://developer.apple.com/app-store/review/guidelines/#app-completeness) require complete review access/live backend; substituting a demo mode for legal/security restrictions requires prior approval under2.1. Guidance retrieved29October2026; no Apple approval is claimed.

Prepare a camera shot showing the real phone/iPad and dedicated Mac, then demonstrate connection/discovery, a synthetic session, one scoped request and the matching response on the owner. Show enough device context to establish physical interaction. Keep OTP entry, inbox credentials, provider keys and unrelated desktops out of frame; disclose any cut around credential entry. Include the actual sharing confirmation, receipt and complete final paragraph. No staged screenshot may impersonate a working connection.

Use separate real-UI screenshots for the listing: catalog with dedicated hosts only, session/reply, supported input, model download/status, resource sample and honest offline state as applicable. Verify supported capture dimensions for the final device set, readable text, no private content, complete cards/composer, correct appearance and no diagnostic overlays. Keep the approved opaque app icon and compare it to the packaged assets. The video and screenshots are evidence/assets, not proof of Apple acceptance.

## Finish and handoff

Ready means the isolated fresh-code rehearsal and all advertised final-build paths passed, the host is operational for review, review notes contain actual values and privacy/support/export/seller decisions are complete. Record exact receipt paths and remaining limitations; do not mark this runbook complete merely because it is written. After review, remove temporary fixture access through supported revocation/deletion under its authorized lifecycle; do not delete personal sessions or change shared production provider keys.
