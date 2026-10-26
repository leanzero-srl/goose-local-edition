# Embedded iPhone/iPad transport proof

Measured 2026-10-26 on two disposable iOS 27.0 simulators using the universal diagnostic app. This proves the embedded transport and diagnostic command protocol against an isolated local owner. It does not prove production LeanZero Link enrollment, actual Goose session controls, physical-device background/cellular behavior, or App Store approval.

## Inputs

- TailscaleKit source: `tailscale/libtailscale` commit `d1dc45d039edb7759ecbee6ba2aea0c4bca884b8`, Go 1.27.1, arm64 simulator. Framework binary SHA-256 `18f40ec92ccb6f46348975f05c0c8ef1cb41bc9685a9a0c51ce02dbec3269642`.
- Swift source `scripts/ios-controller-probe/MeshProbe.swift` SHA-256 `84dad4e6a0dff74df4243adb36c5cb7637efbba0e0ef020c0dbeb4ede3adc21b`. Xcode 27.1 / SDK 27.1; deployment target 18.1. Actual simulator runtime 27.0.
- Go fixture source at `d78635bda`, including the regression where a new connection without a cursor incorrectly replayed an old connection event. A fresh connection now starts at the current tail; explicit cursors retain replay semantics.
- iPhone 17 simulator `400C398A-CD57-434E-B9F2-42F3DB9B2E5B`; iPad Pro 11-inch M5 simulator `BB8DA62F-EA7E-4B20-9A80-FD6EB8B68E5D`. Both were created for this fixture. Its disposable CA was trusted only on these devices. No system trust or production credentials changed.

## Measured results

Both [iPhone](link-ios-mesh-proof/phone.json) and [iPad](link-ios-mesh-proof/ipad.json) completed every extended diagnostic check with `passed: true`. Each started an independent embedded node and received a different mesh address. HTTPS returned the matching random run nonce and owner identity. WSS returned a matching identity envelope. Wrong credentials returned 401; a separate URLSession without the mesh proxy timed out without obtaining the nonce. TLS used ordinary URLSession trust against the disposable test CA, with no ATS exception or trust-all override.

The app persisted and read back a new command UUID before sending. The fixture accepted its harmless synthetic command, invoked a counted delivery sink, then intentionally dropped the acknowledgement. Each app observed the connection-loss error, recovered the acceptance through a status request, and explicitly retried the same command with no second delivery. A different payload under that UUID returned 409. A sibling controller token could neither claim it nor read its status. These sibling-token calls establish ownership isolation, not simultaneous cross-device contention.

After disconnecting before command acceptance, each app requested WSS replay from its stored cursor and received the exact missed command event followed by a later connection event. The [owner request and delivery ledger](link-ios-mesh-proof/requests-final.jsonl) independently records traffic from the two assigned mesh IPs and one delivery per command. The delivery counter is incremented by a sink invocation, not a constant response field.

## Death between acceptance and local journaling

A third command on the iPhone deliberately exited the diagnostic process after its first transport result and before persisting the acknowledgement outcome. The [unfinished receipt](link-ios-mesh-proof/phone-crash-before-relaunch.json) and [persisted command journal](link-ios-mesh-proof/phone-crash-journal-before-relaunch.json) show the checkpoint and no saved interruption. The owner ledger independently records one accepted delivery for that UUID.

On relaunch, the app queried authoritative status before resending. [Recovery](link-ios-mesh-proof/phone-crash-recovery.json) passed, as did status, explicit duplicate retry, payload conflict, ownership and replay checks. The owner still recorded exactly one delivery. The acknowledgement observation deliberately remains `unproven`, so the aggregate receipt is `passed: false`; recovery does not fabricate evidence that the second process could not observe.

An independent source review found the original crash window and the stale-connection replay defect before this run. Both were fixed and exercised. Current-launch node close and ephemeral-state removal passed after the recovery run. The abruptly exited first process did not perform graceful cleanup; its residual private state remains confined to the disposable simulator until that simulator is erased.

## Cleanup and remaining proof

The tracked fixture job `laptop-20261026-163159-105b` stopped cleanly with exit 0 after the stop file was written; [shutdown receipt](link-ios-mesh-proof/fixture-stopped.json) says `failed: false`. Both disposable simulators were shut down. Runtime credentials were never copied to this repository. The diagnostic source and test fixture remain reproducible through their READMEs.

Production authorization, owner execution lifetime, durable command delivery, selected transcript paging, role-based compute exclusion, actual node resources/downloads/provider management, manual fleet sync and full native UI journeys remain separate gates in [the delivery ledger](link-controller-progress.md). In particular, fixture commands execute no model, shell command, or Goose session.
