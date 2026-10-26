# Goose Remote release preparation

Updated 2026-10-26. This is the internal release record, not a submission or a readiness claim. The delivery ledger remains [link-controller-progress.md](../plans/link-controller-progress.md).

## Identity and artifacts

Bundle identifier: `net.leanzero.goose-remote`. Proposed store name: `Goose Remote by LeanZero`. Proposed subtitle: `Your Mac's AI agents, anywhere`. Both fit Apple’s 30-character limits; availability and brand rights still need checking in App Store Connect. These are drafts, not reserved names. Source version/build currently 1.0/1; confirm from the final archive.

Existing Apple team: `ZZ8MTZ6NRZ`. On 2026-10-26 an unrestricted read of the laptop signing identities found one valid `Developer ID Application: Mihai Perdum (ZZ8MTZ6NRZ)`. This confirms that the Mac identity is presently accessible. No iOS distribution identity appeared in that enumeration. It does not establish absence from the developer account. Reuse the membership/team; do not reuse Developer ID as an iOS distribution certificate. App Store Connect access, current agreements, App ID, Apple Distribution/provisioning and reviewer credentials remain unverified. Never copy private keys, account credentials or review passwords into this directory.

Xcode 27.1 (27A9275), Swift 6.4, iOS SDK 27.1; current app deployment target 18.1. Dedicated iPhone and iPad simulator coverage is recorded in `ui/ios/VERIFICATION.md`. A fresh `xcrun devicectl list devices` showed simulated devices only. This is not physical-device qualification.

Pinned embedded transport: libtailscale `d1dc45d039edb7759ecbee6ba2aea0c4bca884b8`. The current privacy-hardened local device and simulator frameworks both passed [verify-mesh-framework.py](../../ui/ios/scripts/verify-mesh-framework.py) on 2026-10-26, including their binary, bundled manifest and pre-construction diagnostic-disable receipts. Device binary SHA256: `61a611339f7cb75599f7f58dcef27b18a72e878c21a2caae2931789ae7e8880a`. Simulator binary SHA256: `e35b1884b0a43fdc5a4d53191dfc52f32918c2eb815c35ace8eefef97b977a77`. These replace the earlier pre-hardening hashes. They identify unsigned dependency artifacts under the local `/tmp/goose-libtailscale-device-build` and `/tmp/goose-libtailscale-ios-build` directories, not a signed app archive or installed-device proof; final packaging needs a fresh receipt.

## Review explanation to substantiate

Goose Remote is intended as a native controller for sessions and jobs on the user’s own Macs. The phone displays structured session events, sends scoped messages and control requests, and requests management operations on those Macs. Execution and model storage remain on the selected Mac. It does not stream the Mac’s rendered screen, execute downloaded model weights on iOS or join the inference pool.

That description is a product-design interpretation, not a determination from Apple. Guideline 4.2.7 restricts certain software-mirroring remote desktop clients, including network and host-account requirements. The native structured-control design must be described honestly in review notes and demonstrated; renaming a remote desktop feature would not exempt it. See [App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/#remote-desktop-clients), refreshed 2026-10-26.

The first release is being prepared as a free companion with no paid feature unlock. No App Store commercial declarations have been submitted. Actual app behavior, host requirements and any third-party AI services must match metadata. Do not claim Apple certification or guaranteed acceptance.

## Concrete package still required

The release receipt must identify the exact source commit, app/build, worker/host versions and capability compatibility; signed archive, export options, dSYM storage and validation result; dependency/license and cryptography assessment; app icon and real iPhone/iPad screenshots; accurate privacy declarations and reachable privacy/support pages; physical-device network/lifecycle/accessibility proof; reviewer authentication and a reachable demonstration host with non-sensitive data. Required external-hardware interaction should be shown in the review video. Reviewers must not need access to the owner’s personal machines or email inbox.

Do not upload placeholder support URLs, an empty privacy declaration, simulated transcript screenshots presented as real functionality, an unsigned framework as an archive, or a list of passing mocks as device evidence. The app manifest declares UserDefaults reason `CA92.1`; its collected-data array remains empty. The embedded framework declares file timestamp `C617.1` and system boot time `35F9.1`. These API reasons do not establish the App Privacy collection answers. The code audit and draft answers are in [DATA-AND-SUPPORT.md](DATA-AND-SUPPORT.md); reconcile them with the actual deployment and final archive. SDK diagnostic upload was disabled before node construction in `44a89aa52d`, but account, device and mesh metadata still leave the device. Inspect the final opaque iOS icon and its packaged assets before upload.

Apple metadata reference: [App information](https://developer.apple.com/help/app-store-connect/reference/app-information/app-information/) and [Manage app privacy](https://developer.apple.com/help/app-store-connect/manage-app-information/manage-app-privacy/), refreshed 2026-10-26. The bundle identifier must match the archive; collection answers include embedded partners. Submission/upload remains a separate external action under the active goal’s release boundary.

## Data, deletion and encryption status

Account deletion is implemented in the worker and native client, including purpose-specific email confirmation and durable recovery (`895702b456`). This is code/test evidence, not a completed production deletion demonstration. A confirmed request revokes access; provider uncertainty stays pending and is not reported as deletion success. Security tombstones, shared email contacts and historical service records still require a justified retention/deletion decision and deployed verification. The [data record](DATA-AND-SUPPORT.md) contains the factual policy/support draft, exact retained categories and remaining inputs; the [deletion runbook](../../leanzero-link/worker/ACCOUNT-DELETION-RUNBOOK.md) contains operational recovery.

The app includes WireGuard/Go cryptography through TailscaleKit as well as Apple networking APIs. The draft export answer is therefore encryption used, including industry-standard encryption outside the Apple operating system. Do not select an Apple-OS-only exemption or set `ITSAppUsesNonExemptEncryption` without the completed classification. France availability and any required declaration are unresolved owner inputs. [Apple encryption documentation requirements](https://developer.apple.com/help/app-store-connect/reference/app-information/export-compliance-documentation-for-encryption).
