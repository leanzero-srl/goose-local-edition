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

/// Live peer dial through the engine's SOCKS5 listener — ignored by default; it joins a
/// REAL tailnet. Point it ONLY at a hermetic control server you started yourself (a
/// local Headscale), never the owner's LeanZero Link account or personal tailnet. The
/// daemon's state lives in a temp dir, never `~/.leanzero`. Env:
/// - `LEANZERO_LINK_LIVE_LOGIN_SERVER` — the hermetic control URL;
/// - `LEANZERO_LINK_LIVE_AUTH_KEY` — a preauth key minted on it;
/// - `LEANZERO_LINK_LIVE_PEER_URL` — `http://<peer mesh ip>:<port>/<path>` served by a
///   second node of that tailnet (bound on the peer's loopback);
/// - `LEANZERO_LINK_LIVE_PEER_EXPECT` — a substring the peer's body must contain.
///
/// Proves the whole fix end to end: the argv's `--socks5-server=127.0.0.1:0` is accepted
/// by the real binary, readiness records the listener the daemon reports, and a peer
/// call through `MeshProxy::http_client` reaches the peer — while a direct dial to the
/// same URL (the pre-fix behavior, the negative control) does not.
#[tokio::test]
#[ignore = "joins a real (hermetic) tailnet; set the LEANZERO_LINK_LIVE_* env and capture the personal tailscale status before/after"]
async fn live_peer_call_goes_through_the_daemons_socks5_listener() {
    use leanzero_link::peer_dial::PeerTimeout;

    let env = |name: &str| {
        std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for this live test"))
    };
    let login_server = env("LEANZERO_LINK_LIVE_LOGIN_SERVER");
    let auth_key = env("LEANZERO_LINK_LIVE_AUTH_KEY");
    let peer_url = env("LEANZERO_LINK_LIVE_PEER_URL");
    let expect = env("LEANZERO_LINK_LIVE_PEER_EXPECT");

    let state = tempfile::tempdir().unwrap();
    let mut config = MeshConfig::new(
        discovery::find_tailscaled().unwrap(),
        discovery::find_tailscale_cli().unwrap(),
        "lzp-live-proxy".to_string(),
    )
    .unwrap();
    config.state_dir = state.path().join("ts");
    config.socket_path = config.state_dir.join("tailscaled.sock");
    config.login_server = login_server;
    config.validate().unwrap();

    let engine = MeshEngine::start(config).await.unwrap();
    let proxy = engine.peer_proxy().await.unwrap();
    eprintln!("live daemon SOCKS5 listener: {}", proxy.addr());
    engine.join(&auth_key, "lzp-live-proxy").await.unwrap();
    eprintln!("live self: {:?}", engine.status().await.unwrap().self_ip);

    let direct = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
        .get(&peer_url)
        .send()
        .await;
    eprintln!("direct dial (negative control): {direct:?}");
    assert!(
        direct.is_err(),
        "the host must have no route to a mesh IP under userspace networking"
    );

    let client = proxy
        .http_client(PeerTimeout::Total(Duration::from_secs(20)))
        .unwrap();
    let body = client
        .get(&peer_url)
        .send()
        .await
        .expect("the peer answers through the mesh proxy")
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    eprintln!("through the proxy: {body:?}");
    assert!(body.contains(&expect), "{body}");

    engine.shutdown().await;
}
