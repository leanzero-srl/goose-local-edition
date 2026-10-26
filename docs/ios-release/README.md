# Goose Remote release preparation

Updated 2026-10-26. This is the internal release record, not a submission or a readiness claim. The delivery ledger remains [link-controller-progress.md](../plans/link-controller-progress.md).

## Identity and artifacts

Bundle identifier: `net.leanzero.goose-remote`. Proposed store name: `Goose Remote`. Proposed subtitle: `Your agents, wherever you are`. Both fit Apple’s 30-character limits; availability and brand rights still need checking in App Store Connect. These are drafts, not reserved names. Source version/build currently 1.0/1; confirm from the final archive.

Existing Apple team: `ZZ8MTZ6NRZ`. On 2026-10-26 an unrestricted read of the laptop signing identities found one valid `Developer ID Application: Mihai Perdum (ZZ8MTZ6NRZ)`. This confirms that the Mac identity is presently accessible. No iOS distribution identity appeared in that enumeration. It does not establish absence from the developer account. Reuse the membership/team; do not reuse Developer ID as an iOS distribution certificate. App Store Connect access, current agreements, App ID, Apple Distribution/provisioning and reviewer credentials remain unverified. Never copy private keys, account credentials or review passwords into this directory.

Xcode 27.1 (27A9275), Swift 6.4, iOS SDK 27.1; current app deployment target 18.1. Dedicated iPhone and iPad simulator coverage is recorded in `ui/ios/VERIFICATION.md`. A fresh `xcrun devicectl list devices` showed simulated devices only. This is not physical-device qualification.

Pinned embedded transport: libtailscale `d1dc45d039edb7759ecbee6ba2aea0c4bca884b8`. Physical-target TailscaleKit framework build job `laptop-20261026-171029-7f8d` exited 0. Its arm64 binary SHA256 is `b69b4f22ac465eeac1f6e725b37dc357142c0598681b4f94354a4acddac56193`; platform iOS, minimum 18.1, SDK 27.1. This is an unsigned dependency build, not an app archive or installed-device proof. The simulator binary hash is `18f40ec92ccb6f46348975f05c0c8ef1cb41bc9685a9a0c51ce02dbec3269642`.

## Review explanation to substantiate

Goose Remote is intended as a native controller for sessions and jobs on the user’s own Macs. The phone displays structured session events, sends scoped messages and control requests, and requests management operations on those Macs. Execution and model storage remain on the selected Mac. It does not stream the Mac’s rendered screen, execute downloaded model weights on iOS or join the inference pool.

That description is a product-design interpretation, not a determination from Apple. Guideline 4.2.7 restricts certain software-mirroring remote desktop clients, including network and host-account requirements. The native structured-control design must be described honestly in review notes and demonstrated; renaming a remote desktop feature would not exempt it. See [App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/#remote-desktop-clients), refreshed 2026-10-26.

The first release is being prepared as a free companion with no paid feature unlock. No App Store commercial declarations have been submitted. Actual app behavior, host requirements and any third-party AI services must match metadata. Do not claim Apple certification or guaranteed acceptance.

## Concrete package still required

The release receipt must identify the exact source commit, app/build, worker/host versions and capability compatibility; signed archive, export options, dSYM storage and validation result; dependency/license and cryptography assessment; app icon and real iPhone/iPad screenshots; accurate privacy declarations and reachable privacy/support pages; physical-device network/lifecycle/accessibility proof; reviewer authentication and a reachable demonstration host with non-sensitive data. Required external-hardware interaction should be shown in the review video. Reviewers must not need access to the owner’s personal machines or email inbox.

Do not upload placeholder support URLs, an empty privacy declaration, simulated transcript screenshots presented as real functionality, an unsigned framework as an archive, or a list of passing mocks as device evidence. The current foundation privacy manifest has empty collection/API lists and is not yet a completed data assessment. Current icon assets from the desktop have transparent rounded margins; inspect final iOS icon assets before packaging.

Apple metadata reference: [App information](https://developer.apple.com/help/app-store-connect/reference/app-information/app-information/) and [Manage app privacy](https://developer.apple.com/help/app-store-connect/manage-app-information/manage-app-privacy/), refreshed 2026-10-26. The bundle identifier must match the archive; collection answers include embedded partners. Submission/upload remains a separate external action under the active goal’s release boundary.
