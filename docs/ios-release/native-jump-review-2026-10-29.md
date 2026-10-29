# Native Jump to latest independent review — 2026-10-29

Verdict: PASS for source and focused-test review. Actual viewport behavior must still be checked by parent before calling the user-visible defect fixed. No confirmed source-level defects found after attempting to refute the change. Scope ONLY SessionDetailView.swift and its scroll-policy tests in OwnerReplayTests.swift; unchanged authentication code was not re-reviewed. No heavy tests or original iPad interactions performed by reviewer.

## Evidence inspected

Complete SessionDetailView.swift, the full diff, added OwnerReplayTests scroll tests and existing scroll-policy/same-work refresh tests; SessionsView.swift selection identity and detail lifecycle. Independently consulted Apple primary ScrollPosition and scrollPosition(_:anchor:) documentation:
https://developer.apple.com/documentation/swiftui/scrollposition
https://developer.apple.com/documentation/swiftui/view/scrollposition(_:anchor:)
https://developer.apple.com/documentation/swiftui/scrollposition/ispositionedbyuser

Apple describes one bound position capable of storing a user-scrolled view ID or an explicit bottom-edge target, with identity stability during data changes. This supports the proposed replacement of the competing ID binding and ScrollViewReader imperative target. It does not alone prove the observed SwiftUI viewport behavior.

## Trace against actual reported failure

Before: user scroll sets visibleMessage to an older row and followsLatest=false. Jump sets followsLatest=true (hides button), then ScrollViewReader writes the bottom sentinel while the independent top-anchored visibleMessage binding remains active. Parent observed the button vanish while viewport stayed oldest. The source contains exactly those two position controllers; their conflict is a plausible causal explanation, not an independently captured internal SwiftUI trace.

After: same user scroll updates scrolling.position. Jump changes that SAME bound position to edge bottom and sets followsLatest=true. There is no remaining visibleMessage binding, ScrollViewReader proxy or sentinel target that can retain the old ID. testJumpReplacesTheBoundReadingAnchorWithTheBottomEdge checks bottom edge AND nil previous view ID; it is stronger than merely checking the follow boolean.

## Rejected suspicions

- Refresh forces a reader back to newest: the chat refresh button does not call jumpToLatest. Same-work refresh retains scrolling state; the transcript-triggered task calls followContent, which leaves the position unchanged when followsLatest=false. The existing top identity anchor remains.
- Older-page prepend itself changes the explicit target: beginOlder disables following; followContent performs no write during/after load until deliberate jump or user scrolling to bottom. New testRefreshAndPrependKeepReadingPositionUntilExplicitJump checks equality of the actual position across each policy transition. Framework preservation under real prepend still requires UI proof.
- Growing same-ID messages no longer follow: .task(id: selected.transcript) still observes complete TranscriptItem values, and followContent directs the same binding to bottom when following. Existing content-change test covers changed text/revision under same ID.
- New session inherits the previous session’s old row: onChange(work) creates a fresh SessionScrollState whose position starts at bottom. SessionsView keeps WorkRef as selection identity. This is the same reset point as before.
- Background/ordinary view updates introduce new reset behavior: no lifecycle hook was added; onDisappear/endSelection and the selectedHistoryReset observer remain in their existing roles. Explicit history reset still goes bottom intentionally. Authentication-driven disappearance/reload and arbitrary older-page recovery are outside this narrow change.
- Programmatic Jump is undone by follow state tracking: onScrollGeometryChange updates followsLatest only while userScrolling; programmatic scrolling does not fall into tracking/interacting/decelerating states by this code’s predicate. No new unconditional geometry-to-position assignment exists.

## Test receipt and remaining proof

Reviewer independently read node-jobs log laptop-20261029-085459-a044: 21 OwnerReplayTests, zero failures, TEST SUCCEEDED; xcresult /tmp/goose-latest-native-derived/Logs/Test/Test-GooseRemote-2026.10.29_08-55-00-+0200.xcresult. Author reports the build linked TailscaleKit and passed strict deep codesign verification; those two extra claims were not independently rerun here. Parent should perform actual long-history user path: initial newest view; scroll to older row; press Jump and observe newest content, not merely vanished button; scroll up again and refresh with same visible row; load older and verify reading position remains. Policy unit tests and Apple API docs are insufficient to substitute for this viewport proof.
