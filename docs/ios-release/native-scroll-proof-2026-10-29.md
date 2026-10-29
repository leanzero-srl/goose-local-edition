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
