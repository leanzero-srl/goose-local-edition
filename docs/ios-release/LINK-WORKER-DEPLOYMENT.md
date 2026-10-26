# Link worker deployment receipt

Observed 26 October 2026, 19:43 UTC. This records the existing auth-service upgrade, not App Store approval or an end-to-end account deletion.

## Source and service

Candidate: `895702b45`. All70 tracked worker files and executable modes on the workhorse were compared against that candidate, with no mismatches or unexpected worker files. Deployed worker tree: `af6f6708c2ce2db2d4594ded0b3790df4f742112`.

Workhorse source receipt commit: `1429b1362e7c3ef3d7d553431bf68e5bfa9ee09a`, on its existing main branch, containing only `leanzero-link/worker`. No push, merge or reset was performed. The unrelated staged `local-edition/skills/goose-mlx-inference/SKILL.md` remained staged at the identical blob `cb5015a0326981da8c663d1064b667d609a7d1a8`. Neither KV nor the registry is in the source commit.

The coordinator activated the staged subtree through the existing `com.leanzero.link-auth` launch agent. It still runs `/Users/workhorse/Projects/goose/leanzero-link/worker/run-node.sh`, reads `/Users/workhorse/.leanzero/link-worker.env`, and uses `/Users/workhorse/.leanzero/link-kv`. The env file/JWT key and proxy configuration were unchanged; the env file predates the former service process, and wrapper, secret parsing and legacy JWT verification sources match the prior deployment. Existing node_modules were retained: all dependency-lock package entries except root declarations matched. No install or secret rotation was needed.

Current service is running, PID55009. Node is24.20.0 with native SQLite; Headscale is0.29.3. The new registry is schema1, mode0600, with0accounts at19:43:38UTC. The KV directory is private and writable.

## Verification

Before activation, the network-disabled target preflight imported the candidate router/Node/SQLite code with an in-memory registry and isolated KV; it passed without external calls or real account writes. Prior source checks passed typecheck,219tests in16files, Wrangler dry-run and independent review. They were not rerun during receipt closeout.

The coordinator's local boundary checks passed health200; controller devices401; deletion status without credentials401; controller Origin403; and GET on the three legacy POST routes405. Independently observed public controller devices/deletion status401 with `Cache-Control: no-store`, and Origin403 at19:36:52UTC. These prove route/auth boundaries, not authenticated phone or deletion workflows.

The public base remains `https://worksmacstudio.tailfc4700.ts.net/leanzero-link`; its existing proxy strips the prefix. Public TLS briefly timed out after activation, before HTTP. It recovered without a change by this lane; cause is unproven. At19:36:19UTC fresh curl from both laptop and workhorse returned health200 with verified TLS. Each of the three resolved public addresses independently returned200. Latest public health at19:43UTC returned200, `ok:true`, `mail:true`, `audience:false`, `mesh:true`, provider `headscale`.

No live OTP, account deletion, provider mutation, Funnel/Tailscale reconfiguration or rollback was performed during this verification/receipt work.

## Recovery and remaining limits

Prior source directory: `/private/tmp/link-worker-before-895702b45`. Preserve the existing KV and SQLite/WAL/SHM files during recovery. Once any deletion is pending/deleted, or a completed deletion receipt exists, the old worker must not be restored: it ignores the new revocation barriers and could accept retired JWTs. Recover with authority-aware code or keep service unavailable. A failed registry read is not evidence that an old-code rollback is safe. Use the worker's [account deletion incident runbook](../../leanzero-link/worker/ACCOUNT-DELETION-RUNBOOK.md) for ambiguous provider outcomes; never force a pending request to completed.

Projsync remains unchanged and scheduled every900seconds. At closeout the workhorse main branch is2commits ahead/7behind its cached origin/main, so current policy does not auto-merge it. The cached repo visibility is public; current projsync blocks automatic pushes for public/unknown repositories. Future source synchronization still needs coordination: goose is not in no_pull, and the Link service is excluded from the busy-process guard, so a future eligible fast-forward could replace source used on the next service restart.

`RESEND_AUDIENCE_ID` remains absent, so current audience sync and segment cleanup are disabled. Historical Link segment ownership and published shared-contact/historical retention policy remain separate owner decisions. Sanity read access was verified; deletion permission and actual account removal were not exercised. This receipt makes no claim that those release boundaries are complete.
