# Gabee final-candidate upgrade hold — 28 October 2026

Result: no installation or restart. The original Gabee app has independently advanced to 3.0.134; replacing it with the planned controller candidate 3.0.132 would downgrade it. Reconcile the controller changes with the newer release before continuing the idle-only rollout.

## Current original runtime

Observed 18:04–18:08 UTC on Gabee via SSH and its authenticated loopback ACP endpoint, using the original cached TLS certificate with hostname verification. Original app PID 65131; original backend PID 65170, loopback port 64992; owned mesh PID 65187. Installed Info.plist reports 3.0.134. Installed embedded CLI SHA-256 is `38ba50802a44bbba744893273bb51ad083fbb3fd97fa1de798346f91b6eda3cc`. Strict deep codesign verification passes; Gatekeeper accepts Notarized Developer ID.

Supported session-activity query at 18:04:44 UTC returned zero running, needsYou, failed, stopped, background, looping and notesWaiting sessions. Supported Link status returned connected, mesh online, no last error, reconnect state reconnected, and live remote execution allowed. Remote execution, chat serving, distributed node permission and execution/MLX wiring all reported true. Link self node is `gabee-macbook-ccd5f4`; owned mesh reports provider node 45 and IP 100.64.0.12. No active work was interrupted.

A single read-only `_goose/unstable/controller/catalog` query returned JSON-RPC `-32601` (method not found) on this running backend. No controller sync, message, inference request, enrollment, retry or permission change was issued. Connected ordinary Link state does not prove controller-protocol compatibility.

## Retained profile observation

Read-only SQLite snapshot: 16 sessions, 37 messages; latest session update 2026-10-25 21:34:47. Complete sorted session-record hash: `2bd9fe3a81f3d2bf235395dce55f1b6d3eca013d170d8b4e667d6f77745a2cf9`. Complete sorted message-record hash: `0f1ded2d8b60f873c5ed656108ab5e6b12da2278b49ffd433ee1b55bf8a8e55b`. Session-ID hash: `90015189f1dd52a28dbc42004e7cc426ed6651698d481b21371e2926094cc591`. Config hash: `d2c1b47d3fa8a6323d97ad9672f4a7b8d43e9c3bc9305dea7743b1c8aae7d551`. Saved identity exists with mode 0600. Remote-execution and remote-jobs settings are true on disk. These are current observations, not before/after upgrade equivalence proof.

The historical attestation helper hardcodes provider node42 and owner295d8723-74c6-49ec-af5c-9e4915786299. Node42 was absent from the current complete provider listing. Adapting the observer to the actual owned-mesh node45 found no exact matching confirmed host enrollment in the inspected controller registry. This is consistent with the missing controller method, but it does not establish why or when the desktop was upgraded. Current controller enrollment and lease are unproved. No registry/provider writes were made.

## Preserved state and staged input

The single existing whole-app rollback at `~/.local/share/goose-upgrade-rollback/Goose Swarm.app` exists and was not overwritten. The planned signed archive was transferred to `/tmp/goose-final-controller-20261028.zip` before the newer installed version was discovered. Its source artifact is the independently reviewed archive identified in `receipt-7a0e558f5.json`; transfer completion alone is not a remote archive hash verification. It was never extracted or installed. Temporary read-only observers were copied into `/tmp`.

No quit, app replacement, original-profile/config write, keyring operation, personal-Tailscale operation, model start, test message, auth bypass or deletion was performed. A protected pre-mutation content snapshot and a clean restart were not needed because the upgrade stopped before any mutation. A future installation must take a new snapshot and repeat the actual idle check. The existing temporary autodownload setting was not changed.
