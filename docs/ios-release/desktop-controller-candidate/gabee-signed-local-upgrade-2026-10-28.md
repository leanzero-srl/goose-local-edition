# Gabee signed local upgrade — 28 October 2026

Status at 21:34 UTC: the complete signed controller candidate launches normally, but saved-login Link recovery fails with `mesh_binding_changed`. The upgrade has not passed its connection/catalog or clean-reconnect acceptance checks. The candidate remains installed and idle for diagnosis. No security override, credential reset, provider-node deletion, or registry mutation was performed by this rollout.

## Artifact and installation

Source includes runtime merge `58e4bd3fc3`. Independent finalizer job `laptop-20261028-232850-91b4` exited zero and its metadata review passed. Archive `/tmp/goose-controller-full-signed-c86a0b33/Goose-Swarm-Apple-submission.zip` is 468,893,965 bytes, SHA-256 `3d124c48fb915f624a7bed4743082d3fbe490671e5be2e48c2a4909c17197c1b`; receipt SHA-256 `c7723a86fb7aed63d1d5a90c66e4ba08d4dea90fb19e54d542e3181d97f05a55`. Signed embedded CLI SHA-256 is `88cdf89cde29c490cccbd0eb4e999b367f2d301aa77eb5120704c509bd30ae0c`.

These hashes were verified again on Gabee before extraction into a fresh installation staging directory. Candidate version is 3.0.134; strict deep signature verification passes. The candidate is Developer ID signed but NOT notarized. The owner authorized local whole-app testing through normal Launch Services; no quarantine removal or Gatekeeper override was used, and no Apple upload occurred in this rollout.

Fresh authenticated activity at 21:31:48 UTC on original backend 65170 reported zero running, needsYou, background, looping, notesWaiting, failed and stopped sessions. The exact original main app 65131 received SIGTERM; all seven original owned processes exited normally before replacement. The complete original 3.0.134 app became the local rollback at `~/.local/share/goose-upgrade-rollback-3.0.134-20261028/Goose Swarm.app`. The older 3.0.132 rollback at `~/.local/share/goose-upgrade-rollback/Goose Swarm.app` was preserved separately.

The candidate was copied as a complete app, installed CLI hash and strict deep signature rechecked, then launched with `/usr/bin/open -a '/Applications/Goose Swarm.app'`. Normal launch produced app 67588 and backend 67641. No profile, keyring, stack or security environment override was introduced. The original installed app's prior CLI was `38ba50802a44bbba744893273bb51ad083fbb3fd97fa1de798346f91b6eda3cc` and passed signature/Gatekeeper checks before being retained.

## Preservation evidence

A 0700 local-only snapshot was saved on Gabee at `~/.local/share/goose-upgrade-snapshots/controller-134-20261028T211658Z`. It contains a consistent SQLite backup, verified with integrity_check, and 14 checksummed config/login/mesh/controller-state files totaling 24,919 bytes; the mesh socket was excluded. Raw contents and credentials were never transferred to another host or recorded here.

Before and after startup, all 16 session records and 37 message records retain their complete sorted stored-column hashes. Sessions: `2bd9fe3a81f3d2bf235395dce55f1b6d3eca013d170d8b4e667d6f77745a2cf9`. Messages: `0f1ded2d8b60f873c5ed656108ab5e6b12da2278b49ffd433ee1b55bf8a8e55b`. Session-ID digest: `90015189f1dd52a28dbc42004e7cc426ed6651698d481b21371e2926094cc591`. Config byte digest: `d2c1b47d3fa8a6323d97ad9672f4a7b8d43e9c3bc9305dea7743b1c8aae7d551`.

Saved login, account secret, link intent, owned tailscaled.state, and every existing controller-host file remain byte-identical to the snapshot. Personal Tailscale's normalized Self/Peer/User/CurrentTailnet identity digest remains `db5fc6a972a85fdfabfd586c426ed3f3d9e3393ac97d998e4113b2c1782b224f`.

Before the swap, owned mesh Self was provider node 45, IP 100.64.0.12, public-node-key hash `66d7dcaf8988209d357c59dbca6194bc60a9a1842536578ce6053e4992156701`. Live remote-execution permission, chat serving, distributed-node permission, and execution/MLX wiring were true. After startup, configured permission and wiring booleans remain true, but live remote execution cannot be claimed while Link is disconnected.

## Failed recovery and remaining proof

The upgraded backend's supported Link status returns loggedIn/reconnectfailed and worker 409 at `/v1/controller/hosts/mesh/context`: code `mesh_binding_changed`, message `Provider host identity changed`. The owned mesh was not started. Activity remains zero.

The existing owner is `295d8723-74c6-49ec-af5c-9e4915786299`, account `0d133a33-717e-4396-9d0c-28c5f83eb7d5`; its local owner/account/host credential/runtime/TLS files are unchanged. Worker `controller/mesh.ts` emits this specific refusal when the confirmed enrollment's provider node ID exists but its exact binding differs. The rollout has not yet compared those authority/provider records, so it does not claim the historical cause. This is not evidence of an expired issued enrollment and does not license blind retries or credential deletion.

The new supported `controller/status` method answers `unavailable`; catalog/lease/controller routing remain unproved. No session command, model request, controller sign-in or sync was issued. A second clean restart is deliberately pending first-connection success. The parent lane owns diagnosis/rollback decision before any merge-success claim.
