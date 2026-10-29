# Native transcript scrolling proof — 2026-10-29

Scope: SessionDetailView scrolling and OwnerReplayTests. Real paired iPad simulator F7078B46… operated by root; this worker did not drive it. No inference, message submission, device archive, distribution export or upload was part of this fix.

## Actual behavior and cause

The original ScrollViewReader proxy competed with a top-anchored scrollPosition ID binding. The jump button changed follow intent but the actual oldest viewport remained unchanged. Commit 15a3625e41 replaced both with one ScrollPosition; root subsequently verified actual oldest-to-final-message movement and refresh retention of the same intra-message lines and screen positions.

The first prepend fix 1026dffd09 passed state tests but FAILED the actual header/Load older path. Its assumption that a header-visible viewport had no bound transcript ID was wrong. The diagnostic-only 4990bf48bd build recorded the real branch; /tmp/goose-prepend-live-trace-0924.log showed boundIndex=0, fallback=false, then 128→237 rows, previousFirstIndex=109, eligible=true, no intervening user input, and no restoration. State tests were not viewport proof.

Commit 035c3f7fc9 measures whether the first transcript row begins at or below the scroll viewport top. It captures that row for a one-use explicit top restoration even when its ID is already bound. Reading inside the first row or another identified message leaves the existing position untouched. Work, request, history reset, cancellation, failed prepend, deliberate user scrolling and Jump guards retain precedence.

## Passing paired-iPad path

On 2026-10-29 at 09:32:42 local time, root selected the long history, scrolled to its header, settled idle and tapped Load older once. The actual screenshot showed the previous first assistant message at 10:54 at the viewport top, followed by the same 10:54 response and body. It did not jump to the newly prepended 10:38 kickoff. Keeping the old header whitespace was not required.

Primary evidence: /tmp/goose-prepend-live-pass-0932.log, source 035c3f7fc9dd672474bf0cdb79cd506b2ecc8bf8. The relevant non-content trace is reproduced here so the proof does not depend on temporary-file retention:

```
09:32:42.122 begin firstBelowTop=true count=128 boundIndex=0 fallback=true
09:32:42.397 loaded count=237 previousFirstIndex=109 cancelled=false sameSelection=true sameReset=true sameWork=true firstExists=true firstChanged=true eligible=true
09:32:42.397 end sameRequest=true didPrepend=true follows=false fallback=true
09:32:42.398 restore changed=true boundMatches=true
09:32:42.410 position count=237 boundIndex=109
```

The root's screenshot inspection establishes viewport movement; the trace establishes why the branch ran. Neither the boolean model nor a compiled binary alone establishes the outcome.

## Validation and boundaries

Before diagnostic removal, guarded job laptop-20261029-092737-74ee passed all 26 OwnerReplayTests with no failures. Result: /tmp/goose-latest-native-derived/Logs/Test/Test-GooseRemote-2026.10.29_09-27-38-+0200.xcresult. The simulator binary explicitly links TailscaleKit and passed strict recursive signature verification. Independent native_auth_review source review found no confirmed blocker. Diagnostics remain temporarily while the separate post-prepend Jump tail-visibility result is investigated.

A separate transient authority-denial alert occurred before the successful replay and is owned by the authentication investigation. This scroll proof does not claim that authentication incident resolved. No device archive is authorized by this receipt.

## Acceptance still open

After the passing prepend, root observed a distinct Jump-to-latest regression at 09:33:34: only the first lines of the last message were visible above the composer, with its final paragraph absent after settling and additional scroll input. Source 035c3f7fc9 therefore is not final scroll acceptance. Primary /tmp/goose-jump-after-prepend-0933.log reports edgeBottom=true and nearBottom=true, which do not establish last-row visibility. Numeric last-row, viewport, content-size and composer geometry are being captured before another correction.

## Numeric tail investigation and clean target

The diagnostic-only 0461eab839 build passed the full-tail screen check at 09:40; this does not erase the earlier 035c3f7fc9 failure. Primary /tmp/goose-tail-geometry-0940.log shows the viewport bottom and composer top both at 970.713 points, with final-row height constant at 343 points. Composer overlap and changed text height are not supported by that trace. On Jump the estimated content height changed from 34608 to 35714 while the actual final-row bottom moved 2393→1287→954 during layout. Geometry observation can affect lazy realization; estimated-edge position and nearBottom were insufficient evidence of actual tail visibility.

The clean follow-up removes all temporary DEBUG logging and diagnostic geometry observers. Initial/latest/follow requests use a typed .latest target on a one-point invisible final row after pending receipts, through the same ScrollPosition binding. Transcript IDs are typed .message(String), so no owner-provided message ID can collide with the tail. Header geometry, explicit prepend restoration and user/reset guards remain. This is targeted hardening against estimated-edge settling; the old failing run cannot prove the new framework behavior. Root must verify the complete final paragraph on the clean build after prepend, plus refresh/reading and initial opening, before closing tail acceptance.

## Clean replay and message identity correction

Root replayed clean 4e673d1d58 on the original paired iPad at 09:54–09:55. Initial opening and post-prepend Jump both showed the entire last 11:05 paragraph above the composer. Prepend itself regressed: header→Load older showed the new 10:38 first row rather than retaining the old 10:54 first row. This failure remains part of the record. At09:56 root also verified Refresh preserved the exact visible10:53 rows and positions: clipped user row, assistant tool request near y385, user row near y482 and assistant body near y603. These tail/refresh passes do not close the separate prepend failure.

The typed-target migration changed the nested row .id but left ForEach using TranscriptItem's raw String ID. The narrow follow-up makes ForEach itself publish the same typed .message identity requested by ScrollPosition, and removes the duplicate nested ID. The actual tail target and all paging/user-intent guards remain unchanged. This resolves a source identity mismatch; real-device prepend proof must establish the runtime result.

## Typed-list result and measured tail following

Root replayed 543b078fb6 at 10:01: prepend PASS, with the prior10:54 assistant row at viewport top near y328, then the10:54 user row near y426 and assistant body near y547. At10:02 the subsequent Jump still FAILED full-tail visibility: the last11:05 row began near y925 and its final retry paragraph remained below the composer near y1185 after settling. Typed list identity fixed the tested prepend path but did not make lazy tail alignment reliable.

The next correction maintains follow intent against actual tail geometry. The existing semantic sentinel measures its bottom relative to the nearest scroll viewport, quantized to physical pixels. Positive overflow can renew the same bound tail request only while following, with no user gesture or older-history load. Content-height changes and returning idle can consume the latest measurement; identical overflow/content-height pairs are deduplicated until the tail becomes visible or an explicit Jump starts a new intent. These are geometry-triggered corrective requests, not a timer. The content-height callback may initially use the last measured overflow; the subsequent actual-tail geometry update refines it. No correction runs while the reader is intentionally away from the bottom.

The combined linked build job laptop-20261029-100921-a1fd passed63 tests:4 ControllerLifecycleTests,15 ControllerTests,16 CredentialRecoveryTests and28 OwnerReplayTests. The test using real SwiftUI ScrollPosition confirms a repeated request for the same semantic tail changes the bound position, so geometry correction is not an equality no-op. Negative tests cover reading, paging and active gestures. The final identical-measurement guard receives a scoped rerun; its receipt is in the implementation commit. Actual clean prepend→complete last paragraph→reading refresh remains required before final acceptance.

## Rejecting estimated layout and corrective feedback

Root replayed 52d17d7cae: prepend PASS at10:15; after natural credential purge/recovery, the selected chat reloaded its latest128 rows, older loading returned237, and Jump at10:16 FAILED to move at all. The old10:54 rows remained at the same positions while the button disappeared. The geometry-correction mechanism therefore did not establish reliable navigation. The attempted eager candidate removed it together with its dedicated policy tests; that candidate was subsequently rejected and the baseline restored as recorded below. Its prior green tests remain recorded above, not represented as user-path proof.

Read-only measurement of the actual paired simulator cache showed237 loaded rows,25,889 text characters total, largest row1,884 characters. The selected transcript is already latest-page bounded with explicit older paging; this change does not fetch more data. The rejected follow-up used a VStack for these already-loaded rows so row heights and the tail exist during layout, rather than depending on LazyVStack estimated extent and later realization. It preserves one typed ScrollPosition, typed ForEach message identity, header-aware prepend restoration and every user/request/reset guard from543b. That experimental cut deleted all52d corrective geometry/cached-overflow state; the baseline restoration restores it pending diagnosis.

This trades layout work proportional to the loaded pages for stable, fully measured targets. The actual237-row payload is modest; histories with many explicitly loaded pages still need responsiveness checks rather than an invented hard cap. The clean actual prepend→Jump full final paragraph→reading refresh path, including recovery-rebuilt transcript, remains the acceptance gate.

## Eager candidate rejected by scale evidence

The eager1024-row test ran in guarded job laptop-20261029-102727-0fc7 on isolatedB7B9. Actual rendered workload:1024 rows,822,762 text characters, content height521,774.667 points. Initial layout13.870 seconds exceeded the5-second test budget; worst measured navigation1.014 seconds exceeded0.5 seconds. A separate first-offset assertion was superseded by the view's initial follow task, a known test synchronization race. That race does not invalidate the genuine layout cost. The eager candidate is rejected and is not shipped; no thresholds or workload were reduced to obtain green.

The working production view was restored to52d while architecture review examines a permanently realized semantic footer outside lazy message rows and the exact same-work renewal/Jump state transitions. No successful scroll acceptance is claimed.

## Writer-only diagnosis of the restored baseline

The next diagnostic build retains52d lazy layout and position behavior. DEBUG-only logs wrap existing explicit writers and the SwiftUI binding setter, preserving the incoming binding transaction. Scroll content appearance/disappearance and existing phase/task/reset callbacks report local random view identity, selection equality, counts and indices; no raw work/message IDs, text or credentials are logged. No new geometry observer is added. The rejected eager test source is preserved in rejected-eager-scale-test-2026-10-29.patch; it is not left as a knowingly racy default test. Its measured performance failure remains above.

Diagnostic job laptop-20261029-103905-c6dc passed28 OwnerReplayTests, with mesh framework linked and deep codesign verification successful. Result: /tmp/goose-latest-native-derived/Logs/Test/Test-GooseRemote-2026.10.29_10-39-06-+0200.xcresult. Independent writer/binding/privacy review passed in /tmp/goose-native-scroll-architecture-review.md. The build includes auth source c1d237d741. This is diagnostic readiness only; the failed same-work recovery/prepend/Jump still requires the parent’s actual trace.

## Controlled target lookup and collection fix

Actual867119 reproduction at10:42:19.135 isolated the failure without renewal: idle237-row view at index109 received Jump, changed message→latest with follows=true/loading=false, and emitted no subsequent binding setter, phase, task or teardown. The request remained unfulfilled; another app writer did not replace it. Root also observed a physical-phone blank transcript on initial opening that a gesture immediately revealed.

The controlled SwiftUI probe starts128 synthetic rows, explicitly moves to old first109, prepends to237 and proves that old first is retained before requesting the target. Job laptop-20261029-105227-041a passed its required preconditions and three-arm result: standalone latest unresolved; ordinary ForEach last-message target rendered its bottom at657.982; latest advertised in the same ForEach rendered its actual tail at658.156. The initial probe104939-c191 falsely failed the message control because viewport height was read only from the unresolved tail; the corrected probe reads native scroll bounds independently. Final viewport assertions additionally subtract adjusted content insets. This is controlled target-resolution evidence, not a claim about undocumented framework internals.

The production change puts typed message rows and the semantic latest row into the same ForEach identity collection, with exactly one VStack per identity. The latest row contains any pending receipt followed by the invisible final point, so bottom targeting still includes that receipt. Lazy rendering remains; eager rendering and geometry-triggered corrective requests are absent. Header-aware prepend restoration, single position binding, user reading guards and reset behavior remain. Existing DEBUG writer logs stay only until actual paired acceptance.

Final collection-target job laptop-20261029-105717-8f08 passed27 OwnerReplayTests, including the grouped-receipt/no-nested-ID probe with inset-adjusted native viewport bounds. Independent final source review passed; deep codesign and TailscaleKit linkage checks passed. The previous diagnostic-only867119 actual failure and every rejected variant remain recorded. Parent original-iPad actual acceptance is still pending for this candidate.

## ae34 actual outcome and architecture decision

Root original-iPad ae34c7c3a7 initial latest and header prepend passed. Jump at11:02:15.166 was acknowledged as latest by the SwiftUI binding at11:02:15.187, but the settled11:06 screenshot still clipped the final523-character11:05 message below the composer; its final retry paragraph was absent. The next gesture traversed message indices232 through109 during2.25 seconds of deceleration, not a logged application restore. Reading Refresh at11:08 preserved exact10:54 positions: body near350, tool request507, response600, following body725 and next tool943. All three passes remain separate from the Jump failure. Primary complete trace: /tmp/goose-scroll-trace-ae34c7.log.

The representative fixture stores only integer word lengths and newline boundaries from237rows/25,889characters. Synthetic words replace all content; no original words, IDs or timestamps are in source. It preserves the abrupt last-row size pattern1806/469/523characters after many55–56-character rows. Job laptop-20261029-111728-759f passed the current top-size-change and following-bottom comparison plus13 SessionSharingTests. Both scroll arms passing means the fixture does NOT explain the actual clipping and does NOT authorize an anchor fix. The actual failure outranks this proxy.

No further SwiftUI scroll retries or anchor variants are planned. Parent authorized a confined native UICollectionView virtualization/scroll coordinator, retaining existing SwiftUI panels, composer, theme and sharing. Independent design attack precedes implementation. Native self-sizing still estimates heights, so stable-ID reading anchors, actual target-cell frame reconciliation and representative237/1024-row real viewport checks are requirements, not assumed benefits of UIKit. Final actual initial/prepend/completeJump/readingRefresh and physical long-message/rejected-receipt paths remain open.
