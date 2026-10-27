# Link worker deployment receipt

Observed 26 October 2026, 19:43 UTC. This records the existing auth-service upgrade, not App Store approval or an end-to-end account deletion.

## Source and service

Candidate: `895702b45`. All 70 tracked worker files and executable modes on the workhorse were compared against that candidate, with no mismatches or unexpected worker files. Deployed worker tree: `af6f6708c2ce2db2d4594ded0b3790df4f742112`.

Workhorse source receipt commit: `1429b1362e7c3ef3d7d553431bf68e5bfa9ee09a`, on its existing main branch, containing only `leanzero-link/worker`. No push, merge or reset was performed. The unrelated staged `local-edition/skills/goose-mlx-inference/SKILL.md` remained staged at the identical blob `cb5015a0326981da8c663d1064b667d609a7d1a8`. Neither KV nor the registry is in the source commit.

The coordinator activated the staged subtree through the existing `com.leanzero.link-auth` launch agent. It still runs `/Users/workhorse/Projects/goose/leanzero-link/worker/run-node.sh`, reads `/Users/workhorse/.leanzero/link-worker.env`, and uses `/Users/workhorse/.leanzero/link-kv`. The env file/JWT key and proxy configuration were unchanged; the env file predates the former service process, and wrapper, secret parsing and legacy JWT verification sources match the prior deployment. Existing node_modules were retained: all dependency-lock package entries except root declarations matched. No install or secret rotation was needed.

Current service is running, PID 55009. Node is 24.20.0 with native SQLite; Headscale is 0.29.3. The new registry is schema 1, mode 0600, with 0 accounts at 19:43:38 UTC. The KV directory is private and writable.

## Verification

Before activation, the network-disabled target preflight imported the candidate router/Node/SQLite code with an in-memory registry and isolated KV; it passed without external calls or real account writes. Prior source checks passed typecheck, 219 tests in 16 files, Wrangler dry-run and independent review. They were not rerun during receipt closeout.

The coordinator's local boundary checks passed health 200; controller devices 401; deletion status without credentials 401; controller Origin 403; and GET on the three legacy POST routes 405. Independently observed public controller devices/deletion status 401 with `Cache-Control: no-store`, and Origin 403 at 19:36:52 UTC. These prove route/auth boundaries, not authenticated phone or deletion workflows.

The public base remains `https://worksmacstudio.tailfc4700.ts.net/leanzero-link`; its existing proxy strips the prefix. Public TLS briefly timed out after activation, before HTTP. It recovered without a change by this lane; cause is unproven. At 19:36:19 UTC fresh curl from both laptop and workhorse returned health 200 with verified TLS. Each of the three resolved public addresses independently returned 200. Latest public health at 19:43 UTC returned 200, `ok:true`, `mail:true`, `audience:false`, `mesh:true`, provider `headscale`.

No live OTP, account deletion, provider mutation, Funnel/Tailscale reconfiguration or rollback was performed during this verification/receipt work.

## Recovery and remaining limits

Prior source directory: `/private/tmp/link-worker-before-895702b45`. Preserve the existing KV and SQLite/WAL/SHM files during recovery. Once any deletion is pending/deleted, or a completed deletion receipt exists, the old worker must not be restored: it ignores the new revocation barriers and could accept retired JWTs. Recover with authority-aware code or keep service unavailable. A failed registry read is not evidence that an old-code rollback is safe. Use the worker's [account deletion incident runbook](../../leanzero-link/worker/ACCOUNT-DELETION-RUNBOOK.md) for ambiguous provider outcomes; never force a pending request to completed.

Projsync remains unchanged and scheduled every 900 seconds. At closeout the workhorse main branch is 2 commits ahead/7 behind its cached origin/main, so current policy does not auto-merge it. The cached repo visibility is public; current projsync blocks automatic pushes for public/unknown repositories. Future source synchronization still needs coordination: goose is not in no_pull, and the Link service is excluded from the busy-process guard, so a future eligible fast-forward could replace source used on the next service restart.

`RESEND_AUDIENCE_ID` remains absent, so current audience sync and segment cleanup are disabled. Historical Link segment ownership and published shared-contact/historical retention policy remain separate owner decisions. Sanity read access was verified; deletion permission and actual account removal were not exercised. This receipt makes no claim that those release boundaries are complete.


## Saved-login compatibility update — 27 October 2026

Applied only the reviewed four-file worker delta from `895702b45` through `23b6f82569`: `hosts.ts`, `routes.ts`, the registry tests and API documentation. Deployed workhorse receipt commit `c56077c882`; prior worker tree matched `af6f6708c2ce2db2d4594ded0b3790df4f742112`, with no local worker edits. Dependencies, environment, proxy, database and credentials were preserved. Restarted only the existing `com.leanzero.link-auth` service. Original Goose runtimes and benchmarks were untouched.

Source checks: full worker suite 229 tests, final focused registry suite 33 tests, typecheck, Wrangler dry-run and independent security review. Live public checks after restart: health 200 with mail=true/audience=false/mesh=true/headscale unchanged; new POST `/v1/controller/hosts/recover-node-secret` without credentials returns 401 invalid_credential and no-store; carrying Origin returns 403 origin_denied and no-store. These prove deployment and refusal boundaries; a live saved-login migration is still pending.

The separate offline-host admission proposal is excluded. The live service still requires current verified host leases before admitting controllers. No original node was removed, no account secret was rotated and no controller gained host authority.


## Approved verified-offline host update — 27 October 2026

The owner explicitly approved implementation and tested deployment. Candidate `5f10daaa5` passed 299 worker tests, typecheck, Wrangler dry-run and independent review (106 rerun cases plus a separate activation/withdrawal/renewal probe). The deployed baseline tree matched `1c3631bf526da860beeac87d89a05c76891adb2c` exactly and had no worker edits. Applied the reviewed eight-file delta, recorded workhorse commit `70bd2a7d50`, then restarted only `com.leanzero.link-auth`. No credentials, registry rows, original profiles or provider nodes were directly edited.

Public smoke: health200 with unchanged capability flags; unauthenticated hosts401/no-store; Origin403/no-store. The running isolated desktop renewed normally and its previously confirmed enrollment gained the exact activation receipt through the live lease route. A real desktop quit then cleared its live lease while preserving that receipt; immediate restart obtained a current lease again. These observations came from read-only registry metadata and the actual test app. The original Forge benchmark stayed running.

Verified sleeping hosts can now permit controller enrollment without advertising endpoints/capabilities or authorizing commands until leased. Unknown legacy nodes still require their own upgrade. Phone admission into the owner's mixed-version fleet remains unproved; no legacy nodes were deleted to bypass it.


## Actionable compatibility refusal — 27 October 2026

Candidate `a431c33ce` changes only two refusal messages and their exact-copy test. Sixteen focused route/adaptor cases passed with unchanged409/update_required and zero key mints; independent review passed. Baseline worker tree `88454eb21a0e5b2f54b76f8ed3b312b75e463ffb` matched the clean deployed subtree. Applied the two-file patch and committed `61ff60ed1b`, then restarted only the existing launch agent. Public `/v1/health` returned200 with unchanged flags, unauthenticated hosts401/no-store. Actual enrolled iPad manual sync displayed the new update/reconnect instructions. Credentials, provider records, authorization predicates and databases were not directly edited.
