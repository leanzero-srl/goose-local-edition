# Original-profile catalog scale proof — 2026-10-27

Measured at 10:06:06 UTC against the installed original laptop backend PID 76225, through its supported authenticated HTTPS ACP connection. Candidate source correction: `7a0e558f5`. This is API proof, not a screenshot or UI-rendering proof.

The measurement initialized an ACP connection, called `_goose/unstable/controller/catalog`, `_goose/unstable/controller/sync`, then `_goose/unstable/controller/catalog`, each with the exact empty parameter object `{}`, and closed only that probe connection. Method declarations were read from `crates/goose-sdk-types/src/custom_requests/controller.rs`; dispatch and projection were read from `crates/goose/src/acp/server/link/controller_client.rs`.

The response is the existing fleet projection: `{updated_at, hosts:[{host:{owner_node_id,name,device_role,protocol_version,availability,last_seen_at,capabilities},entries:[CatalogEntry],stale,error}]}`. Entry titles, paths, contents, credentials, and endpoint details were neither printed nor retained in this receipt.

## Observed result

All three calls returned the same aggregate membership and counts:

- Original laptop `83b81dd1-ec2c-485a-89ec-2d9d43a93d90`: online, stale=false, error=null; **556 entries: 457 chat, 98 benchmark, 1 agent_work**. All 556 work IDs unique; zero entries with another owner ID.
- Gabee `295d8723-74c6-49ec-af5c-9e4915786299`: online, stale=false, error=null; **55 entries: 15 chat, 40 benchmark**. All 55 work IDs unique; zero entries with another owner ID.
- Isolated-profile owner `3437244e-8417-422e-957d-dc0e548d1135`: offline, stale=true, zero entries; explicit error `Owner has no registered mesh endpoint`.

Elapsed end-to-end ACP times on this one warm-cache measurement:

- Initial catalog read: **54.524 ms**.
- Supported fleet synchronization: **1,079.060 ms**.
- Following catalog read: **56.663 ms**.

Fleet freshness advanced from `2026-10-27T10:06:01.883505Z` to `2026-10-27T10:06:06.422860Z`. The original laptop's 457 chat entries match the separately established 455 user + 2 scheduled visible-session population. This measurement did not query or modify the original sessions database; its 73,324-session / 384,284-message retention checks belong to the separate original-profile upgrade verification.

## Persistence observation and limits

The existing `~/.local/share/goose/sessions/controller-desktop/catalog.json` was 323,641 bytes before and after the sync. Its bytes and modification time changed, and the in-memory `hosts` content also compared unequal across the call despite identical aggregate counts. Therefore this sample **does not prove unchanged-cache write suppression**, nor does it show an unchanged-content write regression. The background sync pump remained running; no attempt was made to freeze it or infer which concurrent refresh wrote the file.

This was a warm synchronization and cache-read proof. It did not measure cold snapshot duration, individual owner HTTP page count, or UI rendering latency. The query-plan and deterministic persistence proofs remain in the correction's isolated test gate. No session messages, providers, models, profile selection, app process, or mesh daemon were changed by this probe.

Ephemeral aggregate receipt: `/tmp/goose-original-catalog-scale-receipt.json`. Probe helper: `/tmp/goose-original-catalog-scale-probe.py` (uses the existing initialization/authentication/HTTPS/SSE handling; retains no credentials).
