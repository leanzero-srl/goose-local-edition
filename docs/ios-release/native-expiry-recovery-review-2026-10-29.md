# Native expiry recovery adversarial review — 2026-10-29

Current verdict: PASS for expiry recovery security/concurrency, including the follow-up correction on disk after 0cc332b4e8. No unresolved security blockers. No product edits, test launches or device actions performed by reviewer. Older-page reading-position restoration remains a user-path limitation, described below.

## Primary material inspected

/tmp/goose-ipad-auth-diagnosis.md; complete ControllerStore.swift, changed and existing ControllerTests.swift, ControllerAPI.swift refresh validation, LocalControllerStore persistence and owner retention, OwnerHTTPControllerService selected/history/stream behavior, SessionDetailView.swift scroll state and conditional rendering. Reviewer independently read node-jobs log laptop-20261029-084537-5594: 112 tests, 1 skipped, 0 failures; TEST SUCCEEDED. Result bundle /tmp/goose-latest-native-derived/Logs/Test/Test-GooseRemote-2026.10.29_08-45-39-+0200.xcresult. No independent rerun was performed.

## Confirmed defects, corrected before approval

P1 — Ordinary manualSync authority 401/403 retains cached authorized content. Scenario: an existing selected transcript/catalog was fetched legitimately, then device revocation or scope removal causes /hosts to reject the still-unexpired bearer. validCredential returns it without rotation; manualSync stops streams, /hosts throws, and the outer catch only sets stale/error. entries, selectedWork and persisted catalogs/selected remain. A subsequent denied stream cannot purge them because manualSync already stopped every stream. This was independently traced and corrected: manualSync captures epoch outside do, guards the outer catch against stale callbacks, and passes access failures to the shared purge/recovery handler. The new testAuthorityDenialWithoutRotationPurgesSavedViewsAndStopsAfterOneRecovery covers the exact scenario and persistent failure after one renewal. Successful-rotation purge did not refute the original defect: the scenario specifically did not rotate first.

P1 — Host removal left selected transcript in memory. The author identified this sibling and reviewer independently verified it: retainOwners removed disk state, but select restored previous selectedWork before checking that the work/host still appeared in the new catalog. Corrected by assigning previous only after the membership guard; testRemovedHostCannotKeepSelectedCachedTranscript covers visible and persisted removal.

The follow-up also resets recoveryAttempted upon a newly verified device enrollment, reports access failures visibly, and tests that refresh503 after denial never restores purged content. These changes were reviewed.

## Rejected suspicions

- Concurrent original owner closes causing repeated refresh: first denial increments accessEpoch and cancels all streams synchronously. Other old callbacks either see cancellation or fail the epoch gate; recoveryTask/refreshTask coordinate the single remaining attempt.
- Late old opening or stream frame replacing fresh state: captured epoch plus cancellation gate prevents catalog writes, work writes and recovery from old streams. selectedRequest also changes at every purge.
- select/history catch passes current epoch and could invalidate replacement content: rejected because every epoch-changing authorization purge also replaces selectedRequest, and both catch paths require the captured selection request to remain current.
- Successful refresh revives old cached content: rejected. Successful refresh changes epoch, stops streams, clears visible/persisted authorized views, then queues fresh authority discovery and owner reads. A replacement token alone never reinstalls the purged selected snapshot.
- Fresh owner 403 repeatedly rotates: recoveryAttempted remains set; an unexpired replacement cannot trigger another automatic recovery. Later actual expiry may authorize another lifecycle renewal, which is intended, not a tight rotation loop.
- queueCredentialSync self-await deadlock: rejected. It waits for an already-captured current sync and refresh from a separate task; neither waits on credentialSyncTask. When rotation originates in manualSync, this may perform one extra read sync but has no await cycle or recurring token rotation.
- Recovery resends user mutations: rejected. The recovery path only refreshes credentials and calls manualSync/select read paths. submit remains explicitly reached through delivery; checkDelivery only reads the receipt. Existing saved refresh request IDs are not cleared on a lost response.
- Background rotation reconnects streams: rejected. queueCredentialSync and stream starters require foreground, and sceneBackground cancels the queued sync and stream tasks.

## User-path limitation

Selection identity survives purge. Arbitrary older-page reading position is not proven preserved: clearAuthorizedViews discards paged transcript; recovery owner.selected loads latest history only. SessionDetailView retains visibleMessage in State, but that ID may no longer be in the newly loaded transcript. No added path refetches older pages to restore it. This does not justify retaining unverified cached content, but the release must not claim older-history anchor restoration without a real user-path check. Parent informed.

## Scope of approval

The code meets the requested fail-closed revocation/scopes/persistent-denial behavior, fences obsolete stream callbacks, coordinates refresh, reconnects on ordinary rotation, preserves refresh receipt retry identity, and adds no automatic mutation resend. This is a code/test review; original-device end-to-end validation remains the parent’s work. Do not broaden this PASS to older-page anchor restoration.
