# Independent review of initial App Store / SEO draft

29 October 2026. Scope: store-metadata.en-US.draft.json and STORE-METADATA-RECEIPT.md. Read-only source and receipt review; no publication, live model calls, suite execution, or source edits. Verdict: **HOLD draft for one wording correction**. This is not publication approval.

## Confirmed defect

1. High-confidence factual/privacy overclaim in JSON description (line 8): “Model storage and inference stay on your Macs; your phone and tablet remain controllers.” The unqualified inference claim is false for cloud-provider sessions. The primary ipad-live-proof-2026-10-29.json records session_direct_openrouter=true, absent host override, and three actual cloud responses. Source crates/goose/src/providers/openrouter.rs defaults to https://openrouter.ai (line 493) and sends chat/completions requests (line 839). The final paragraph acknowledges external AI services but does not cure the earlier contradictory location claim. A reader could reasonably infer local-only AI processing. Replace with wording such as: “Downloaded models are stored on the selected Mac. Your phone and tablet control the work; sessions use the AI provider configured on their owner Mac.” Keep the existing external-AI/tool sharing paragraph. Model downloads may use mounted/shared storage, as the actual laptop-target download resolved to the workhorse SSD; do not promise physical storage exclusively inside the selected Mac.

## Rejected suspicions / supporting evidence

- Fabricated native send/steer claims: rejected. ipad-live-journeys-2026-10-29.md describes actual iPad simulator Send and Steer, visible sharing consent and Delivered status. The primary JSON independently records all three receipts delivered once, one occurrence of each payload, interrupted original output and the 80-item replacement. These prove the installed bf5966 desktop / older native combination, not the newer final build or physical-device qualification.
- Background continuity unsupported: rejected for draft wording. Actual journey put iPad Home during replacement, owner continued and returned idle; reopening received the result. ControllerStore.sceneBackground (lines 782-788) stops local streams/sync and suspends mesh, without sending an owner work-cancel command. Actual force-quit and physical-device lifecycle proof are not established by these receipts and remain release verification work.
- Download/progress fabricated: rejected. Native UI showed owner acceptance and 30%; ipad-download-proof-2026-10-29.json records owner delivered receipt, tracker done and byte/file evidence. The draft promises starting and checking progress, not a proven final Done screen. That screen remains unverified.
- Provider-management invented: rejected as a draft feature statement, but keep the publication hold. ManagementView.swift lines 124-184 implements configuration/create; OwnerManagement.swift reads providers and rejects secret values in responses at line 86. ManagementModel.swift lines 81-85 keeps the retry envelope in device-bound Keychain. Actual final-build provider-management journey is still unproven, exactly as the embedded hold says.
- Benchmark keyword unrelated: rejected. Native WorkKind includes benchmark, with presentation in ControllerModels.swift lines 4-9. Keyword relevance is source-backed; no ranking/search-volume promise is made.
- iPhone/iPad and themes invented: rejected. Xcode targets family 1,2 and source has adaptive size-class layouts; StudioAppearance.swift implements system/light/dark. Do not confuse source support with complete physical-device acceptance.
- Wrong metadata counts: rejected. Independently parsed exact current JSON: name24, subtitle30, keywords72, promotion126, description1498; matches receipt. No duplicate keyword tokens found. Parent supplied the current Apple-limit recheck; this review did not independently reclassify legal/export obligations.
- Accidental publication or seller-ready claim: rejected. status=draft-not-submitted and explicit seller/App Store record/export holds. Receipt expressly calls this preparation, not accepted release evidence.
- Draft credentials/privacy leakage: no credential values or personal test excerpts found in these two draft files. Dedicated demo account and mobile-specific privacy/support publication remain explicit holds. Do not reuse the personal proof JSON or screenshots as public captures.
- New latest-history build treated as proven: rejected. latest-history-verification-2026-10-29.md explicitly distinguishes native409 / desktop574 focused/full/build verification from outstanding real UI positioning. Caption “Read the latest” remains conditional on that hold. Existing bf5966 notarization does not qualify574; draft claims neither final-build proof nor Apple acceptance.

## Result after correction

The inference-location correction is the only confirmed copy defect. After changing it and recomputing the receipt length count, this initial draft can receive a draft-only PASS while retaining all publication holds. No publication authorization is given by this review.

## Correction verified — 29 October 2026

Re-read the current JSON and receipt. The confirmed inference-location defect is resolved by: “Downloads use the selected Mac’s configured storage. Sessions use that Mac’s configured AI provider; your phone and tablet remain controllers.” The misleading prior sentence is absent. Independently recomputed description length is1552, matching the updated receipt. The wording accommodates configured/mounted storage and local or cloud AI providers without asserting local-only processing.

Updated verdict: **PASS for the initial draft only**. The six publication holds and draft-not-submitted status remain. This supersedes the wording HOLD above; it does not approve publication or qualify the unverified final-build journeys. No other review scope was reopened.
