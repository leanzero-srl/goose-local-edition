//! LeanZero Link — goose-owned embedded Tailscale mesh sidecar.
//!
//! # Isolation invariant (load-bearing — never weaken)
//!
//! This crate runs its OWN `tailscaled` in userspace-networking mode: its own state
//! directory (default `~/.leanzero/tailscale/`), its own unix socket
//! (`~/.leanzero/tailscale/tailscaled.sock`), `--tun=userspace-networking` (no system
//! TUN device, no root). It NEVER touches `/var/run/tailscale*`, the system state
//! directories, or any personal/system Tailscale daemon that may be running on the same
//! machine. [`mesh::MeshConfig::validate`] enforces this by refusing system paths, and
//! every daemon it spawns is terminated per-pid — never by process group.
//!
//! The one contact with the personal tailnet is DATA-PLANE only ([`tailnet_route`]): a DNS
//! query to MagicDNS (`100.100.100.100`) for a `*.ts.net` server name, and a TCP dial of
//! the tailnet address it answers — the packets any app on this Mac sends when MagicDNS
//! is wired. Its socket, state, CLI and lifecycle stay untouched.
//!
//! Auth keys are injected strings minted elsewhere (the LeanZero Link worker); the node
//! token is derived locally ([`token::node_token_from_secret`]) from the per-account
//! secret the worker issues with the join key. This crate never talks to any auth
//! backend and never verifies the account JWT.
//!
//! The [`control`] module is the `/v1/swarm` node-to-node service: `GET /nodes`,
//! `GET /sessions`, and the `GET /stream` WebSocket, fed by a [`state::SwarmStateSource`]
//! (implemented later by goose-server) and by the [`state::PeerRegistry`] peer fabric
//! built on [`mesh::MeshStatus`] / [`mesh::MeshPeer`]. Userspace networking gives the
//! host no route to mesh IPs, so every OUTBOUND peer call goes through the daemon's
//! loopback SOCKS5 listener via [`peer_dial`] — never a direct dial.

pub mod control;
pub mod control_proxy;
pub mod discovery;
pub mod identity;
pub mod inference;
pub mod intent;
pub mod manager;
pub mod mesh;
pub mod netpath;
pub mod peer_dial;
pub mod pubsub;
pub mod replica;
pub mod state;
mod subprocess;
pub mod tailnet_route;
pub mod token;
pub mod wire;
pub mod worker_client;
