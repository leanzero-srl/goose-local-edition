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

The approved offline-host update is deployed (worker candidate `5f10daaa5`, workhorse receipt `70bd2a7d50`). A previously activated exact provider binding remains verified while offline; routing and commands still require a current lease. Unknown legacy hosts are not exempt from upgrade. Old rows gain activation only through their next genuine verified lease, never backfill.

27 October 2026: Written after the real account was blocked by four original legacy nodes while the isolated desktop owner connected successfully. Signed iOS export and an isolated desktop response are proven separately; neither establishes phone-to-owner operation.


## Development launch isolation

`facf47ca35` implements `GOOSE_SKIP_PROTOCOL_REGISTRATION=1` only for unpackaged dev-server launches. Set it for test/development upgrades so they do not replace the installed app's goose:// handler. Isolated profiles additionally use `GOOSE_PATH_ROOT`, `GOOSE_USER_DATA_DIR` and `GOOSE_DISABLE_KEYRING=1`; an authorized original-profile upgrade omits all three to retain its own store and keyring. Keep the verified CLI outside the signed app and supply the owned mesh binary paths explicitly.

A terminal-launched development app needs its terminal input kept open; Electron Forge exited when its input closed. Use the repository's pinned pnpm10.30.3 and hoisted configuration rather than an unrelated fallback executable. If launched over SSH, remove SSH_CONNECTION/SSH_CLIENT/SSH_TTY from this app's environment to match a normal desktop launch; otherwise the private userspace mesh CLI can issue a misleading SSH-disconnection prompt. Do not add a production accept-risk flag or touch personal Tailscale.

## Original Gabee upgrade receipt — 27 October 2026

The owner approved original-profile upgrades only while idle, including controller access with existing settings. Gabee's original backend restarted while packaging completed, so the earlier no-backend observation was discarded. Immediately before quit, the actual authenticated ACP session-activity response showed zero running, background, looping and waiting sessions. Only the exact original app received SIGTERM; its backend and owned mesh exited normally. No benchmark was interrupted.

The complete notarized app archive was verified on Gabee before installation. Archive SHA-256: `520c8596fc790e8e4431e378aacef412458d905a19c7701209829a87ec9e94e0`; signed embedded CLI SHA-256: `788ddf7e19b22f52eca6326e5fc511631b2d8687a80bced6561345dad8ca700a`. Strict deep signature verification and Gatekeeper assessment passed. The complete old app became one designated local rollback; no signed component was replaced in place. Launch Services opened the new app normally without quarantine changes.

The launch retained the original profile and keyring. `GOOSE_DISABLE_AUTO_DOWNLOAD=1` was the only temporary application override; no profile redirection or `RUST_MIN_STACK` workaround was supplied. The original 16 sessions and 37 messages survived: sorted session-ID and complete message-record digests matched before and after. Permission values and the full config digest also matched. No account code was requested or message sent by this upgrade.

The original mesh retained its provider node ID and IP while its node key changed during verified enrollment. The local socket's resulting key matched the authoritative provider record and the worker's exact confirmed host binding, activated binding and current runtime lease. Personal Tailscale's self and peer identity fields were unchanged. These checks used read-only provider/registry inspection, without inference-controller polling. Raw local receipts, credentials and private session contents are deliberately excluded from this repository.

For later idle checks, the tested observer uses the original backend's loopback listener and its cached TLS certificate as the trusted CA, including hostname verification. ACP HTTP initialization returns a connection ID; subsequent requests return 202 and their result arrives on that connection's SSE stream. Query `_goose/unstable/session_activity/get`, emit counts only, then DELETE the observer connection. Never log raw transport exceptions, URLs or headers containing local credentials.

### Two-physical-Mac proof

At 09:25 UTC, the upgraded original Gabee desktop's supported ACP controller facade synchronized the real catalog, selected the existing isolated laptop test work, and opened its current snapshot. The command was built from that returned WorkRef, store generation, revision and run state, with one UUID idempotency key. Its live provider observation was `claude-acp`; the sharing descriptor explicitly did not report a provider origin, so this receipt makes no endpoint claim.

Gabee sent one authorized continuation to the existing laptop test session. The receiver returned `accepted`, and querying the same command's receipt returned `delivered`; it was not resubmitted. The coordinator independently observed the prompt and the exact assistant reply, “Remote message from Gabee received.”, in the laptop's actual desktop UI. Execution used the laptop's already configured Claude session. The sender was Gabee's actual desktop ACP controller facade, not a remote browser UI test. No message was sent into an original Gabee session, and its providers and permissions were not changed. This proves cross-Mac catalog/history access and command delivery plus visible response; it does not claim a phone-to-Mac run.
