# App Store preparation

Checked 2026-10-26. Reopen the live Apple sources before each submission: this is a procedure, not a permanently valid policy snapshot.

## Review the actual product

Read the relevant sections of the [App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/) against actual behavior. Native remote control, remote desktop mirroring, hosted software, AI chat, VPN, and a paid companion are different classifications. Record the applicable sections and unresolved interpretation; do not assume a category from the word “remote”.

For remote products, determine where downloaded code/models execute. Running on a user's remote host differs from executing downloaded app functionality on iOS, but disclose the real behavior to review. Do not apply the remote-desktop LAN restriction to every API client without analysis, and do not assert that a native UI guarantees exemption. Review hosted/chatbot provisions if applicable. Check AI data disclosure and consent before sending personal data to third-party AI providers.

Payments are a separate product decision. Analyze what is purchased, where, which functionality it unlocks, and applicable storefront rules. Do not assume “free companion” or bring-your-own API key is automatically exempt from all commerce rules. A free tool with no purchase/unlock flow is a different case from a paid service companion. Recheck live guidance, including 3.1 and any regional terms, before adding prices, purchase links or entitlements.

## Submission readiness

Check existing Apple release records before asking the user to enroll again. A Developer Program team already used for macOS can also cover iOS/iPadOS. Separate membership/team, upload authentication and signing: a Developer ID Application certificate/notarization workflow is for Mac distribution outside the App Store; iOS needs its own explicit App ID and appropriate distribution signing/provisioning. Current App Store Connect access and roles still need verification. Apple permits reuse of supported Apple ID/app-specific-password credentials for `notarytool` and `altool`; this does not prove that an App Store Connect API key or iOS signing assets exist. See [certificate types](https://developer.apple.com/help/account/certificates/certificates-overview/), [membership coverage](https://developer.apple.com/programs/whats-included/), and [TN3147](https://developer.apple.com/documentation/technotes/tn3147-migrating-to-the-latest-notarization-tool). Inspect credential names/configuration without exposing secret values; never print or source secret files merely to establish identity.

Resolve and record these against the actual build:

| Area | Required evidence or decision |
| --- | --- |
| Toolchain | Current [upcoming requirements](https://developer.apple.com/news/upcoming-requirements/) and [submission guidance](https://developer.apple.com/app-store/submitting/), upload SDK, deployment target and supported device families. Never freeze “latest Xcode” into a permanent rule. |
| Developer account | Correct organization, Team ID, bundle ID, roles, agreements, and signing capability. Never silently switch teams or accept agreements on someone's behalf. |
| App record | Version/build, truthful description/category/age-rating answers, support/privacy URLs, iPhone/iPad screenshots of the actual build. No invented functionality. |
| Markets and rights | Intended storefronts, current business/trader declarations and regional requirements, seller identity, licensed branding/assets and dependency notices. Recheck [EU trader requirements](https://developer.apple.com/help/app-store-connect/manage-compliance-information/manage-european-union-digital-services-act-trader-requirements/) when distributing in the EU; do not assume the certificate's personal name establishes an individual or organization account. |
| Access for review | Working dedicated review account, all necessary host/service access, exact steps and explanations of remote functions. Keep backend reachable; do not require the reviewer to own the developer's personal Mac or intercept an emailed OTP. Supply required authentication codes through the review notes securely. |
| Permissions | Purpose strings reflect actual use; requested entitlements and background modes have real functionality. Denial produces an understandable usable state. |
| Privacy | Data map, third-party SDK behavior, retention/deletion, privacy policy and accurate App Privacy declarations. Inspect SDK collection too. |
| Security | Auth isolation, credential handling, revocation, no secret diagnostics, validated transport, authorization for remote writes. |
| Export | Complete the [export compliance determination](https://developer.apple.com/help/app-store-connect/manage-app-information/overview-of-export-compliance/) for the shipped cryptography and embedded networking. Do not set `ITSAppUsesNonExemptEncryption` by guesswork. |
| Beta | TestFlight build and tester access, external beta review when applicable, feedback/crash owner. A TestFlight pass is not final App Store approval. |

If legal/security obligations prevent a demo account, follow Guideline 2.1(a)'s prior-approval route for a fully featured built-in demonstration mode. Review notes alone do not establish that approval. For required external hardware, provide the physical-device pairing/interaction video requested by Apple's [complete-review guidance](https://developer.apple.com/help/app-review/before-submitting-for-review/complete-review/), not only a screen recording. Explain the services, AI providers and business model used by the app.

If account creation is supported, provide the applicable in-app account-deletion initiation flow. Explain what data is deleted/retained and any legitimate retention. Logging out or disabling an account is not deletion. Verify backend deletion and token/device revocation, not just disappearance of a screen. See [account deletion requirements](https://developer.apple.com/support/offering-account-deletion-in-your-app/).

Use [App Privacy Details](https://developer.apple.com/app-store/app-privacy-details/) to classify collected data, linkage, tracking, and purposes. Data processed only on device differs from data transmitted to the developer or partners. Review the exact flow; “encrypted” does not automatically mean “not collected”. Keep the privacy policy consistent with the app, backend, diagnostics, and support tools.

Inspect [privacy manifests](https://developer.apple.com/documentation/bundleresources/privacy-manifest-files) and [third-party SDK requirements](https://developer.apple.com/support/third-party-SDK-requirements/). Declare required-reason API use with the appropriate approved reason; a manifest does not replace the privacy policy or App Privacy answers. Review dependency signatures/required manifests for the actual embedded SDK versions.

For accessibility declarations, use [Accessibility Nutrition Labels](https://developer.apple.com/help/app-store-connect/manage-app-accessibility/overview-of-accessibility-nutrition-labels/) honestly: disclose only capabilities actually tested. Do not equate app launch under VoiceOver with complete accessibility.

## Archive and release

Archive from a pinned commit using the release scheme. Validate signing, entitlements, bundled assets, SDK dependencies, build number, privacy material and export answers on the actual archive. Retain the corresponding dSYMs and release receipt in approved storage. Use the repository's established export/upload process; inspect the current Xcode export options instead of copying an obsolete plist.

Use [TestFlight](https://developer.apple.com/testflight/) with representative iPhone/iPad users and real networking/backend conditions. Track known issues and exactly which build fixes them. Prepare the final review notes and demo access before any submission approval is needed. An approved action stays approved; do not create redundant checkpoints.

If rejected, read the exact message, reproduce on the submitted build, compare the cited current rule, then correct or appeal with concrete evidence. Never hide functionality from review, add unannounced capabilities after approval, or misrepresent a remote service as local-only.

Use the [support operations procedure](operations.md) for rollout and incidents. Release notes describe the user-visible result; omit internal security-sensitive implementation details. Keep submission credentials, support accounts and API keys outside skills and source control.
