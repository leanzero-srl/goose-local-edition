//! Fixture tests for the pure surfaces: `status --json` parsing (fixture shape captured
//! read-only from a real tailscale 1.98.5 daemon, IPs scrubbed to 100.64.0.x), backend
//! state mapping, argv construction, and binary discovery error text.

use std::path::{Path, PathBuf};
use std::time::Duration;

use leanzero_link::discovery::{discover, DiscoveryError};
use leanzero_link::mesh::{
    parse_status_json, BackendState, MeshConfig, MeshError, MeshStatus, DEFAULT_LOGIN_SERVER,
};

const RUNNING_JSON: &str = include_str!("fixtures/status_running.json");
const NEEDS_LOGIN_JSON: &str = include_str!("fixtures/status_needs_login.json");

fn test_config() -> MeshConfig {
    MeshConfig {
        tailscaled_path: PathBuf::from("/opt/leanzero/bin/tailscaled"),
        tailscale_cli_path: PathBuf::from("/opt/leanzero/bin/tailscale"),
        state_dir: PathBuf::from("/home/lz/.leanzero/tailscale"),
        socket_path: PathBuf::from("/home/lz/.leanzero/tailscale/tailscaled.sock"),
        hostname: "lz-node-self".to_string(),
        login_server: DEFAULT_LOGIN_SERVER.to_string(),
        tag: None,
        startup_timeout: Duration::from_secs(30),
        join_timeout: Duration::from_secs(90),
        cli_timeout: Duration::from_secs(15),
    }
}

#[test]
fn parses_running_status_with_peers() {
    let status = parse_status_json(RUNNING_JSON).unwrap();
    assert_eq!(status.backend_state, BackendState::Running);
    assert!(status.online);
    assert_eq!(status.self_hostname.as_deref(), Some("lz-node-self"));
    assert_eq!(
        status.self_ip.as_deref(),
        Some("100.64.0.1"),
        "must prefer IPv4"
    );

    assert_eq!(status.peers.len(), 2);
    let a = &status.peers[0];
    assert_eq!(a.hostname, "lz-worker-a");
    assert_eq!(a.ip.as_deref(), Some("100.64.0.2"));
    assert!(a.online);
    assert_eq!(
        a.last_seen, None,
        "zero-time LastSeen (connected peer) must map to None"
    );

    let b = &status.peers[1];
    assert_eq!(b.hostname, "lz-worker-b");
    assert_eq!(b.ip.as_deref(), Some("100.64.0.3"));
    assert!(!b.online);
    assert_eq!(b.last_seen.as_deref(), Some("2026-08-30T11:22:33Z"));
}

#[test]
fn parses_needs_login_status() {
    let status = parse_status_json(NEEDS_LOGIN_JSON).unwrap();
    assert_eq!(status.backend_state, BackendState::NeedsLogin);
    assert!(!status.online);
    assert_eq!(status.self_ip, None);
    assert_eq!(status.self_hostname.as_deref(), Some("lz-node-self"));
    assert!(status.peers.is_empty());
}

#[test]
fn backend_state_maps_all_documented_names_and_keeps_unknown() {
    for (name, expected) in [
        ("NoState", BackendState::NoState),
        ("InUseOtherUser", BackendState::InUseOtherUser),
        ("NeedsLogin", BackendState::NeedsLogin),
        ("NeedsMachineAuth", BackendState::NeedsMachineAuth),
        ("Stopped", BackendState::Stopped),
        ("Starting", BackendState::Starting),
        ("Running", BackendState::Running),
    ] {
        assert_eq!(BackendState::from(name.to_string()), expected);
        assert_eq!(expected.as_str(), name);
    }
    assert_eq!(
        BackendState::from("FutureState".to_string()),
        BackendState::Other("FutureState".to_string())
    );
}

#[test]
fn mesh_status_wire_shape_round_trips() {
    let status = parse_status_json(RUNNING_JSON).unwrap();
    let wire = serde_json::to_string(&status).unwrap();
    let back: MeshStatus = serde_json::from_str(&wire).unwrap();
    assert_eq!(back, status);
    assert!(
        wire.contains("\"backend_state\":\"Running\""),
        "backend_state must serialize as a plain string: {wire}"
    );
}

#[test]
fn garbage_and_empty_status_are_loud_parse_errors() {
    let err = parse_status_json("not json at all").unwrap_err();
    assert!(matches!(err, MeshError::StatusParse { .. }), "{err}");
    assert!(err.to_string().contains("not json at all"), "{err}");

    let err = parse_status_json("").unwrap_err();
    assert!(err.to_string().contains("<empty>"), "{err}");
}

#[test]
fn tailscaled_argv_uses_verified_isolation_flags() {
    let argv = test_config().tailscaled_argv();
    assert_eq!(argv[0], "/opt/leanzero/bin/tailscaled");
    assert!(
        argv.contains(&"--tun=userspace-networking".to_string()),
        "{argv:?}"
    );
    assert!(
        argv.contains(&"--statedir=/home/lz/.leanzero/tailscale".to_string()),
        "{argv:?}"
    );
    assert!(
        argv.contains(&"--socket=/home/lz/.leanzero/tailscale/tailscaled.sock".to_string()),
        "{argv:?}"
    );
    assert!(
        argv.contains(&"--no-logs-no-support".to_string()),
        "{argv:?}"
    );
}

#[test]
fn up_argv_carries_socket_key_hostname_and_pins_settings() {
    let mut config = test_config();
    config.tag = Some("tag:leanzero".to_string());
    let argv = config.up_argv("tskey-auth-abc", "lz-node-self");
    assert_eq!(argv[0], "/opt/leanzero/bin/tailscale");
    assert_eq!(
        argv[1], "--socket=/home/lz/.leanzero/tailscale/tailscaled.sock",
        "--socket is a global flag and must precede the subcommand"
    );
    assert_eq!(argv[2], "up");
    assert!(
        argv.contains(&"--auth-key=tskey-auth-abc".to_string()),
        "{argv:?}"
    );
    assert!(
        argv.contains(&"--hostname=lz-node-self".to_string()),
        "{argv:?}"
    );
    assert!(
        argv.contains(&"--accept-routes=false".to_string()),
        "{argv:?}"
    );
    assert!(
        argv.contains(&format!("--login-server={DEFAULT_LOGIN_SERVER}")),
        "{argv:?}"
    );
    assert!(argv.contains(&"--reset".to_string()), "{argv:?}");
    assert!(argv.contains(&"--timeout=90s".to_string()), "{argv:?}");
    assert!(
        argv.contains(&"--advertise-tags=tag:leanzero".to_string()),
        "{argv:?}"
    );
}

#[test]
fn up_argv_omits_tags_when_unset() {
    let argv = test_config().up_argv("tskey-auth-abc", "lz-node-self");
    assert!(
        !argv.iter().any(|a| a.starts_with("--advertise-tags")),
        "{argv:?}"
    );
}

#[test]
fn status_and_logout_argv() {
    let config = test_config();
    let status = config.status_argv();
    assert_eq!(
        status,
        vec![
            "/opt/leanzero/bin/tailscale".to_string(),
            "--socket=/home/lz/.leanzero/tailscale/tailscaled.sock".to_string(),
            "status".to_string(),
            "--json".to_string(),
        ]
    );
    let logout = config.logout_argv();
    assert_eq!(logout[2], "logout");
}

#[test]
fn validate_refuses_system_tailscale_paths() {
    let mut config = test_config();
    config.socket_path = PathBuf::from("/var/run/tailscaled.socket");
    let err = config.validate().unwrap_err();
    assert!(matches!(err, MeshError::UnsafeConfig { .. }), "{err}");
    assert!(err.to_string().contains("/var/run"), "{err}");

    let mut config = test_config();
    config.state_dir = PathBuf::from("/var/lib/tailscale");
    assert!(config.validate().is_err());

    let mut config = test_config();
    config.state_dir = PathBuf::from("/Library/Tailscale/state");
    assert!(config.validate().is_err());

    assert!(test_config().validate().is_ok());
}

#[test]
fn discovery_env_override_that_points_nowhere_is_a_hard_error() {
    let err = discover(
        "tailscaled",
        "LEANZERO_TAILSCALED",
        Some(PathBuf::from("/nonexistent/tailscaled")),
        &[],
        &[],
    )
    .unwrap_err();
    assert!(
        matches!(err, DiscoveryError::EnvOverrideMissing { .. }),
        "{err}"
    );
    let text = err.to_string();
    assert!(text.contains("LEANZERO_TAILSCALED"), "{text}");
    assert!(text.contains("/nonexistent/tailscaled"), "{text}");
    assert!(text.contains("refusing to fall through"), "{text}");
}

#[test]
fn discovery_not_found_lists_every_location_searched() {
    let err = discover(
        "tailscaled",
        "LEANZERO_TAILSCALED",
        None,
        &[PathBuf::from("/pathdir-one"), PathBuf::from("/pathdir-two")],
        &["/known/a/tailscaled", "/known/b/tailscaled"],
    )
    .unwrap_err();
    let text = err.to_string();
    for expected in [
        "LEANZERO_TAILSCALED",
        "/pathdir-one",
        "/pathdir-two",
        "/known/a/tailscaled",
        "/known/b/tailscaled",
    ] {
        assert!(text.contains(expected), "missing '{expected}' in: {text}");
    }
}

#[cfg(unix)]
#[test]
fn discovery_finds_executables_but_not_plain_files() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("tailscaled");
    std::fs::write(&plain, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(discover(
        "tailscaled",
        "LEANZERO_TAILSCALED",
        None,
        &[dir.path().to_path_buf()],
        &[],
    )
    .is_err());

    std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o755)).unwrap();
    let found = discover(
        "tailscaled",
        "LEANZERO_TAILSCALED",
        None,
        &[dir.path().to_path_buf()],
        &[],
    )
    .unwrap();
    assert_eq!(found, plain);

    let known = discover(
        "tailscale",
        "LEANZERO_TAILSCALE_CLI",
        None,
        &[],
        &[plain.to_str().unwrap()],
    )
    .unwrap();
    assert_eq!(known, Path::new(plain.to_str().unwrap()));
}
