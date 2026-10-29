# Remote compute-node management

29 October 2026. The owner's question exposed a missing original requirement: Add a Mac / Node setup supplied enrollment instructions but did not create desktop compute-node definitions. Keep physical host enrollment on that Mac; a controller cannot install or enroll an absent machine by inventing a registry entry.

## Native implementation

Nodes → selected Mac → Compute nodes reads the owner inventory and supports creating cloud, configured-endpoint and owner-local MLX definitions. Provider/model choices and installed complete local models come from that owner. Save creates a definition only: no inference, loading, session rerouting or defaults changes. User-owned definitions can be removed with their exact observed definition fingerprint; the owner retains usage/refusal checks. Pool-managed definitions remain read-only. Old owners show an explicit update-required message when the new route is absent.

The existing owner/device/store-bound management outbox and receipts are reused. Unknown delivery keeps the original immutable UUID/payload. Accepted is not Applied. Both immediate delivery and a later Check delivery refresh the node inventory; failed refresh preserves the authoritative Delivered receipt but clears the stale list and displays the read failure. The creation form reads current capabilities/models and dismisses only for its own delivered command.

Independent source review in /tmp/goose-native-node-review.md identified and root repaired two issues before rollout: delayed delivery originally left stale inventory, and the form initially captured stale provider/capability data. Reviewer confirmed both source repairs. The accepted-to-delivered regression asserts one transmission and refreshed inventory; failed-refresh and removed-capability/model cases are covered.

## Verification

Focused native job laptop-20261029-134213-f5eb passed17 ManagementTests with0 failures, using isolated simulator B7B9DE08-2F64-4573-8562-EC0F7C6FF2A9 and actual linked TailscaleKit from /tmp/goose-libtailscale-ios-build/Build/Products/Release-iphonesimulator. Full log inspected: no errors or invalid-size warnings. Existing StudioIcon actor-initializer and test outboxAccount actor-isolation warnings remain, plus expected no-AppIntents metadata notices; this is not a warning-free build claim.

Earlier job133948-20af failed before tests because the operator passed a nonexistent simulator framework directory. It was corrected without source workarounds; the failure remains in the job ledger.

At the initial source commit this receipt covered native source and focused tests only. The subsequent native installation is recorded below. Rust execution gates, an upgraded owner and physical create/read/remove proof remain outstanding. Main and the previously accepted Apple desktop43a artifact remain unchanged.

## Existing management proof corrected

Download completion was already visually verified: latest-history-verification-2026-10-29.md records Applied on the Mac and79.3MB Ready, after the earlier receipt had left that final screen pending. Supported cleanup removed the disposable model. Provider creation/configured state/removal and active steering were already proved on the original-account iPad simulator against the real Mac; see ipad-provider-proof-2026-10-29.json and desktop-controller-candidate/ipad-live-journeys-2026-10-29.md. These are not physical-iPad claims. Physical iPhone cold Send/reply proof is separate.

At13:30 the physical iPhone refreshed the original laptop's resource sample: CPUload/core0.38, memoryavailable53.8%, capacity128GiB, with a fresh relative timestamp. Model inventory was readable in portrait and landscape, then portrait was restored. No model loaded or new download requested. A physical iPad was still absent from devicectl; Goose Controller iPad is the simulator.

Final native follow-up: optional local_models_error preserves a named local-storage failure while cloud/endpoint nodes remain available. MLX creation is unavailable on that failure even if a stale model ID remains selected. Independent source review passed; focused job laptop-20261029-134459-c6d7 passed18/0 with no full-log errors or invalid-size warnings. No real node has yet been created by this candidate.

Native install follow-up13:46–13:50: source8762c69d386e/native tree130a11f36007 archived in job laptop-20261029-134632-0de6, exit0. Strict signing, actual phone membership, pinned mesh framework/privacy manifests and matching app dSYM passed. Installed in place on the original physical iPhone and launched through its normal icon; saved sign-in and catalog remained. Compute nodes against original laptop43a visibly says “Update Goose Swarm on this Mac to manage compute nodes from your controller.” No creation control is offered. See native-compute-nodes-device-2026-10-29.json. This proves old-owner compatibility, not successful node creation. The first install command used a mistyped archive suffix and failed before installation; the corrected exact archive path succeeded. No desktop upgrade or model inference occurred.
