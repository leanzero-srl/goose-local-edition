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
//! Auth keys are injected strings minted elsewhere (the LeanZero Link worker); this
//! crate never talks to any auth backend.
//!
//! A `control` module (the `/v1/swarm` service) lands in a later pass and will build on
//! [`mesh::MeshStatus`] / [`mesh::MeshPeer`] as its wire shape.

pub mod discovery;
pub mod mesh;
mod subprocess;
