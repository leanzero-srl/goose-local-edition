# Existing Goose host upgrade for mobile controllers

A controller joins the same account mesh as desktop hosts. An older host discovers all account peers and can send its shared compute bearer to them. It must therefore be upgraded before a controller is admitted. A new isolated test owner does not upgrade an original host, including an offline original with no node expiry.

## Preserve work and identity

1. Attribute every blocking provider node to its own machine using its owned mesh socket and persistent identity. Names alone are not sufficient when a live status comparison is available. Read the account registry and provider inventory without printing credentials.
2. Inspect the actual installed binary and running processes. Updating source or the worker does not update the loaded desktop backend. Legacy hosts without the controller registration and peer-admission paths have no hot migration capability.
3. Prove the original app is idle before replacing its runtime. A benchmark is owned by its original app even when its executor runs elsewhere. Do not interrupt the app, owned mesh or benchmark fixtures. Queue behind the real workload; do not bypass resource admission.
4. Upgrade the complete signed app through its normal installation path, or use development Electron with the verified external `GOOSE_BINARY`. Never replace a binary inside an existing signed `.app`.
5. Retain the original profile on that machine. If the original used no `GOOSE_PATH_ROOT`, leave it unset. Setting it to the user's home redirects configuration and sessions to different locations; setting it to a test root creates another owner. Never run two backends over the original store and mesh state concurrently. Never copy credentials between physical machines.
6. Preserve existing inference/job/management permissions. A protocol upgrade is not permission to enable remote management of original sessions.

## Saved-login path and proof

The new host registers with its own saved account JWT, obtains its own host credential, and recovers the existing account secret through `/v1/controller/hosts/recover-node-secret`. The response must match account, owner and email. Missing or corrupt remote secrets and invalid local secret files remain errors. Logout during recovery must not recreate a credential. Revoked or expired logins require ordinary authentication; no new code is needed when the saved login is still valid.

The host then reauthenticates its existing mesh state with a one-use provider-verified host key, confirms its exact provider binding, renews its lease and installs restricted peer admission. A separate metadata-only attestation while the old binary still runs is unsafe: it does not change that binary's legacy peer behavior.

Record before/after node ID, machine/node key hashes, IP and preauth binding without recording secret values. Prove exactly one usable node for the original Mac, original sessions retained, permissions retained, controller exclusion, real lease, and immediate clean quit/reconnect. An unexpected extra legacy node requires diagnosis; do not delete it to make a test pass. A fake mesh test does not prove Headscale reauthentication preserves identity.

The current deployed admission rule requires every usable host to have a verified current lease. The separately reviewed offline-host proposal is pending explicit approval and is not deployed. It would not exempt unknown legacy hosts from upgrade.

27 October 2026: Written after the real account was blocked by four original legacy nodes while the isolated desktop owner connected successfully. Signed iOS export and an isolated desktop response are proven separately; neither establishes phone-to-owner operation.
