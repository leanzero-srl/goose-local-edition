# Original-host enrollment recovery

Canonical procedure snapshot from `~/.claude/skills/goose-feature-dev/SKILL.md`.

## Original-host Link recovery (2026-10-27)

The controller TLS listener is separate from ordinary swarm control. `ControllerHostFactory::listen_address` defaults to 127.0.0.1:41227; changing `LEANZERO_LINK_CONTROL_PORT` does not move it. When an original app upgrade fails with address-in-use, attribute that listener to its exact process and profile. Quit the obsolete isolated app through its normal lifecycle; do not change original settings or leave two backends over one store.

For an incomplete issued mesh enrollment, a consumed one-use key can leave local and provider node keys different. Do not diagnose the first cause from that difference alone. Capture the actual daemon argv, original state path, live public-key hash when available, persisted private-derived public-key hash in memory, provider node/user/preauth IDs, and worker binding/lease. Never emit private keys. Before an authorized retry, take a local-only 0700/0600 snapshot of config and owned mesh state; never transfer it to another Mac or park it on the AI SSD.

The existing expiry path can recover this state: prove the worker's issued enrollment has expired; one ordinary UI Retry then reaches context-without-active-enrollment, gets the named `enrollment_expired` response for its saved pending ID, and creates one new enrollment through the normal authenticated endpoint. This is not permission to loop retries, manually mint a key, delete a provider node, alter state, or attest a binding by hand. Exact confirmation, activation, lease and peer admission still decide success. A transport error or a mismatched still-live binding is not positive expiry evidence.

Measured on the original laptop: one such retry retained provider node33, its machine identity and IP, persisted a new node key matching live Self/provider/confirmed/activated binding, and obtained a real lease. Config bytes and every original session/message record remained unchanged; personal Tailscale identity projections stayed equal. Only current/old private node-key fields changed in the owned state. The first failed attempt's root cause remains unproved. Isolated stale-state experiments did not all reproduce the real outcome; do not generalize them into a product fix. After success, verify normal UI and clean quit/reconnect separately.

Detailed rollout receipt: `docs/plans/link-controller-host-upgrade.md`. No host/controller wire contract or authority change was required.
