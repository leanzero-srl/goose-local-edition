//! Live isolation test — ignored by default because it starts a REAL `tailscaled`.
//!
//! What it proves: the discovered system binaries accept the exact argv this crate
//! builds, a goose-owned userspace daemon comes up on its own socket under
//! `~/.leanzero/tailscale/`, reports `NeedsLogin` (no auth key -> it must NOT join
//! anything), and shuts down per-pid with the state dir left intact.
//!
//! Isolation: own state dir, own unix socket, `--tun=userspace-networking` (no TUN, no
//! root), WireGuard port auto-selected. It never touches `/var/run/tailscale*` or any
//! personal daemon. Run it only with the personal `tailscale status` captured before
//! and after, and compare the identity fields:
//!
//! ```sh
//! tailscale status --json > /tmp/personal-before.json
//! cargo test -p leanzero-link --test live_isolated -- --ignored --nocapture
//! tailscale status --json > /tmp/personal-after.json
//! ```
#![cfg(unix)]

use std::time::{Duration, Instant};

use leanzero_link::discovery;
use leanzero_link::mesh::{BackendState, MeshConfig, MeshEngine};

fn process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[tokio::test]
#[ignore = "starts a real goose-owned userspace tailscaled; run explicitly with the personal-daemon before/after capture"]
async fn live_userspace_daemon_reaches_needs_login_and_shuts_down_clean() {
    let tailscaled = discovery::find_tailscaled().unwrap();
    let cli = discovery::find_tailscale_cli().unwrap();
    let config = MeshConfig::new(tailscaled, cli, "leanzero-link-live-test".to_string()).unwrap();

    let home = dirs::home_dir().unwrap();
    assert!(
        config.state_dir.starts_with(home.join(".leanzero")),
        "live test refuses to run outside ~/.leanzero: {}",
        config.state_dir.display()
    );
    assert_ne!(
        config.socket_path.display().to_string(),
        "/var/run/tailscaled.socket"
    );
    config.validate().unwrap();

    let state_dir = config.state_dir.clone();
    let engine = MeshEngine::start(config).await.unwrap();
    let pid = engine.pid().await.unwrap();
    eprintln!("live tailscaled up: pid {pid}");

    // A fresh daemon may pass through NoState before settling on NeedsLogin.
    let deadline = Instant::now() + Duration::from_secs(20);
    let state = loop {
        let status = engine.status().await.unwrap();
        assert_ne!(
            status.backend_state,
            BackendState::Running,
            "no auth key was given — the daemon must NOT have joined anything"
        );
        assert!(!status.online);
        assert!(status.peers.is_empty());
        if status.backend_state == BackendState::NeedsLogin || Instant::now() >= deadline {
            break status.backend_state;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    eprintln!("live backend_state: {state}");
    assert_eq!(state, BackendState::NeedsLogin);

    engine.shutdown().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !process_alive(pid),
        "live daemon pid {pid} survived shutdown"
    );
    assert!(
        state_dir.join("tailscaled.state").exists(),
        "state file must survive shutdown for fast re-login"
    );
}
