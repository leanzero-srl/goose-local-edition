# Terminal rejected receipts — independent review 2026-10-29

Verdict: SOURCE + TARGETED TEST PASS. No confirmed remaining defect in this cut. Physical modal observation and broader backend build/lint gates remain distinct; no device UI or test reruns performed by reviewer.

Production diff reviewed: ControllerStore.deliver and checkDelivery replace unconditional error clearing with one shared rejectionMessage(receipt). Only rejected produces an error, using the recorded reason or neutral review fallback. GooseRemoteApp lines77–79 already presents this property in its existing alert. Accepted, dispatching and delivered retain nil error; delivered still clears only a draft equal to submitted text. Receipt validation and durable recording precede the message. No command ID, command payload, receipt, draft, retry path, or journal semantics are changed.

Test checks both immediate rejection and lost acknowledgment followed by receipt lookup, retaining original command ID/payload/rejected receipt and an independently edited draft. It confirms sharing is resolved, rejected is no longer unfinished, exactly one submit occurred, and another checkDelivery does not poll or submit. Existing delivered and stale-revision sharing tests remain in the full class.

Primary red receipt independently read: laptop-20261029-111329-a8cb failed the two error-message assertions in both branches, four failures, as expected on old implementation. Primary green receipt independently read: laptop-20261029-111728-759f SessionSharingTests 13 tests / zero failures; combined14 / zero failures / TEST SUCCEEDED (the other test is the independent scroll probe). xcresult /tmp/goose-latest-native-derived/Logs/Test/Test-GooseRemote-2026.10.29_11-17-29-+0200.xcresult.

Reviewed source SHA256 ControllerStore dc8e277a5fdd5932abb860404c5798f89ae33e18944d8959d092a83b08972118; SessionSharingTests 4728843424bdbb1adb3a411a40d7bfc115aec5ebca3fb54a2081bc9a5cae5a6d.

Rejected suspicion: rejected receipt panel is not removed. SessionDetailView renders the last pendingCommands record for this work independently from unfinishedCommand. The fix adds visible terminal error rather than restoring a nonexistent missing panel.
