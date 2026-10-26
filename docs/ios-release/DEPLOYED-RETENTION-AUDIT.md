# Goose Remote operational retention audit

Observed 2026-10-26, 21:10–21:13 UTC. Read-only, bounded audit on workhorse. No config changes, OTP, account deletion, provider writes, registry/KV content inspection, personal-content inspection or heavy jobs. Operational log reads were bounded to the last 128 KiB of the Link log and last 4 KiB of its rotation output; only housekeeping event names/counts were emitted. Secret file inspection emitted presence booleans only. This is runtime/config evidence, not approved retention policy or proof of deletion.

## Confirmed runtime and deployed configuration

- `com.leanzero.link-auth` is loaded/running, PID 55009, runs=1, never exited. Launch agent executes the existing worker `run-node.sh`; both stdout and stderr target `/Users/workhorse/.leanzero/link-server.log`.
- Loopback health returned `ok:true`, capabilities `mail:true`, `audience:false`, `mesh:true`, mesh provider `headscale`. Health is an observed runtime capability declaration, not an exercised email/provider write.
- Env presence: Resend API key present; Resend audience ID absent. Headscale API URL/key/login-server all present. Sanity project/dataset/token all present. Tailscale API key/tailnet absent. No secret values were returned. Sanity configuration presence is not independent proof of event delivery or deletion permission. The deployment receipt separately says Sanity read access was verified.
- Headscale config declares SQLite, log level `info`, embedded DERP server disabled. Its stdout/stderr path is `/Users/workhorse/.leanzero/headscale/serve.log`. No backup/retention/rotation/snapshot-named config keys were discovered by the bounded key parser. Embedded DERP disabled does not mean no relays are used, and this audit did not enumerate actual selected relays or their operators.

## TTL sweep: activity observed; coverage is narrower than all account data

The deployed Link log is mode 0600 and 2,379 bytes at inspection. Twelve most recent matching housekeeping events were `fs_kv_swept`, each `removed:0`; no sweep error was in the matching output. These lines have no timestamps and do not prove each was emitted by the current PID. They do prove sweep activity exists in the live service log; this is not a demonstration of deleting a populated expired record.

Deployed and local SHA-256 match for `src/node-server.ts`, `src/lib/fs-kv.ts`, `src/lib/config.ts` and `ops/rotate-link-log.sh`. Matched node code calls a sweep at startup and every `RATE_WINDOW_SECONDS` (3,600 seconds). It only removes expired filesystem KV records, leaving non-expiring node secrets alone. Registry challenge/history/tombstone retention is a separate path and is not purged by this sweep.

**Document correction applied:** `DATA-AND-SUPPORT.md` previously said rate windows have a one-hour TTL. The rate-limit window is one hour, but `src/lib/ratelimit.ts:30` stores `expirationTtl: windowSeconds * 2`: a two-hour KV TTL for a one-hour rate window. Expired files can remain until the next sweep. Do not equate the request window, stored TTL, and physical deletion time. Legacy OTP validity/TTL is 600 seconds; native registry challenge validity is not a purge schedule.

SHA-256 receipts:
- node-server.ts: `1f347a7731cfeb5a969792ef7cf460eb8f42752688d528eac3bb26bfdea298dd`
- fs-kv.ts: `c2e54fd0ae2c7fd6e334fa96a3e59ab1b59d54f9b0268e03511506231d4b0240`
- config.ts: `f2bf52b6cb749d5bd3e9ba7bf919b1c0d485208f888598e4888dcb1f71ddf236`
- rotate-link-log.sh: `6bba76961af606ae2cca61d386a71eb976b0a32c9e90abf9c1951f40eb660e54`

## Log rotation: installed and successful activity observed

`com.leanzero.link-log-rotate` exists and is loaded. Its calendar schedule is daily at 03:17 host local time. It was idle (normal for a scheduled one-shot), runs=1, last exit code=0. It invokes the matched repository rotation script. No `RETAIN_DAYS`, `LINK_LOG`, or `LINK_LOG_ARCHIVE` overrides appear in its plist or current launchd environment.

The matched script defaults to 30 and deletes archives with `find ... -mtime +30`. It copy-truncates the active log and creates mode0600 archives under a mode0700 directory. This is a day-rounded archive-age predicate, not exact erasure 30 days after each event; scheduled execution and the duration of the active log also matter.

The archive directory has 31 matching files, with oldest mtime 2026-09-26 00:17:05 UTC and newest 2026-10-26 01:17:04 UTC. Rotation output mtime is 2026-10-26 01:17:04 UTC; its last five recognized successes report archive counts 31,32,32,31,31. This closes the specific question of whether the rotation job is configured and has run successfully. It does not prove deletion from backups, host snapshots or provider-side systems. Archive contents were not read. Headscale/proxy log retention was not established.

## Bounded backup discovery

Inspected plist metadata in workhorse user LaunchAgents (74 plists, 4 unparseable), system LaunchAgents (1 plist, readable) and system LaunchDaemons (3 plists, 1 unparseable). Matching labels/arguments for backup/restic/borg/duplicacy/Headscale/Link identified only Headscale, Link auth, and Link log rotation. The unparseable plists mean this is not a complete system inventory or proof of no backup job.

Top-level metadata in `~/.leanzero` includes proxy configuration backup names (`funnel-backups`, funnel/serve backup JSON files), a benchmark-poster backup name, Link log archive and MCP log archive. These were not opened and do not establish a backup of account databases. No backup/snapshot-named entry was found in the worker ops directory or the actual `~/.leanzero/headscale` directory. The guessed `~/.config/headscale` and Homebrew Headscale config directories do not exist; the loaded plist identifies the actual config location.

The readable `/Library/Preferences/com.apple.TimeMachine.plist` had no `AutoBackup` value and zero `Destinations` entries. This narrow configuration observation is insufficient to conclude Time Machine/host snapshots/off-host backups do not exist. No repository of database backups, backup schedule, expiry policy or tested restore was established. Backup configuration remains **not established**, with its discovery scope now documented rather than presumed absent.

## What remains unknown and should not be promised

Resend sent-email history and contact retention; Sanity transaction history and downstream copies; actual relay/proxy provider retention; Headscale historical log retention; registry/tombstone purge policy; independent cloud-AI providers and their content retention; host/system/provider backups and copies. Neither source defaults nor this health check answer those questions. No populated account deletion or recovery workflow was exercised. No claim of legal retention basis, exact global deletion deadline, policy publication or App Store compliance follows from this audit.

Inputs read: `docs/ios-release/DATA-AND-SUPPORT.md`, `LINK-WORKER-DEPLOYMENT.md`, worker node/KV/config/rate-limit/rotation sources; selected live launchd configuration/state; env presence; service health; bounded housekeeping events; metadata of known service directories. No repo or skill file was changed.
