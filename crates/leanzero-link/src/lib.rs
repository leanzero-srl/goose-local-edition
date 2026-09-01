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
//! Auth keys and node tokens are injected strings minted elsewhere (the LeanZero Link
//! worker); this crate never talks to any auth backend.
//!
//! The [`control`] module is the `/v1/swarm` node-to-node service: `GET /nodes`,
//! `GET /sessions`, and the `GET /stream` WebSocket, fed by a [`state::SwarmStateSource`]
//! (implemented later by goose-server) and by the [`state::PeerRegistry`] peer fabric
//! built on [`mesh::MeshStatus`] / [`mesh::MeshPeer`].

pub mod control;
pub mod discovery;
pub mod identity;
pub mod manager;
pub mod mesh;
pub mod pubsub;
pub mod state;
mod subprocess;
pub mod wire;
pub mod worker_client;
