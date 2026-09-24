//! REMOTE SINGLE: this Mac's MLX chat served by the single engine on ANOTHER Mac, reached through
//! LeanZero Link's chat inference proxy.
//!
//! The goosed that starts the route owns a [`leanzero_link::inference::InferenceRelay`] — a
//! loopback, capability-URL listener that forwards the engine's OpenAI paths to the peer's
//! `/v1/swarm/inference/*` through the mesh proxy — and publishes the route (its pid, the relay's
//! base URL, the peer, the served id, the peer's admission cap) under the state dir, because each
//! desktop window runs its own goosed and every one of them must route the same way. Readers
//! never trust the record blindly: a dead pid is `Stale` (named, ignored), an unparseable file is
//! `Unreadable` (routing refuses to guess). Whether the peer's engine actually serves is decided by
//! probing the relay's `/v1/models`, exactly like a local engine's port.
//!
//! While a route is up: the swarm router's MLX node is the peer's engine (one `mlx-remote` node,
//! its own provider instance at the relay's base URL, capacity = the peer's cap, in-flight = the
//! peer engine's own `/v1/status`), this Mac's `mlx-sidecar` node is not a candidate, and the
//! `omlx` provider's `OMLX_HOST` follows the relay (`align_omlx_host_env`).

use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;

use anyhow::{Context, Result};
use leanzero_link::inference::InferenceRelay;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::config::paths::Paths;

const RECORD_FILE: &str = "mlx-remote-route.json";

/// The route as every goosed on this Mac reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishedRoute {
    /// The goosed that owns the relay — the only process that can stop it.
    pub pid: u32,
    /// The relay's base URL (`http://127.0.0.1:<port>/relay/<capability>`). Carries the
    /// capability: never logged whole.
    pub base_url: String,
    /// The peer as the caller named it (a Link node id).
    pub peer: String,
    pub peer_hostname: String,
    pub model_id: String,
    /// `served_model_id` over the PEER's settings — the id every chat request carries.
    pub served_model_id: String,
    /// The peer's admission cap (`maxConcurrentRequests` from its status).
    pub capacity: u32,
    /// The thinking choices of the peer's profile for the served model (the kwargs a local
    /// sidecar node would read from THIS Mac's profile). `None` = nothing to send.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_kwargs: Option<Map<String, Value>>,
}

impl PublishedRoute {
    /// The route's id in the swarm pool and in every reason the router names.
    pub fn node_id(&self) -> String {
        format!("remote:{}", self.peer_hostname)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RouteRecord {
    Absent,
    Mine(PublishedRoute),
    Other(PublishedRoute),
    Stale(PublishedRoute),
    Unreadable { path: PathBuf, error: String },
}

impl RouteRecord {
    /// The live route, whichever goosed on this Mac owns it.
    pub fn live(self) -> Option<PublishedRoute> {
        match self {
            Self::Mine(route) | Self::Other(route) => Some(route),
            Self::Absent | Self::Stale(_) | Self::Unreadable { .. } => None,
        }
    }
}

pub fn record_path() -> PathBuf {
    Paths::in_state_dir(RECORD_FILE)
}

pub fn read() -> RouteRecord {
    read_at(
        &record_path(),
        std::process::id(),
        super::mlx_distributed_owner::pid_alive,
    )
}

pub(crate) fn publish_at(path: &Path, route: &PublishedRoute) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension(format!("json.{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(route)?)
        .with_context(|| format!("writing {}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("restricting {}", tmp.display()))?;
    }
    std::fs::rename(&tmp, path).with_context(|| format!("renaming onto {}", path.display()))
}

pub(crate) fn withdraw_at(path: &Path, self_pid: u32) -> Result<bool> {
    match read_at(path, self_pid, |_| true) {
        RouteRecord::Mine(_) => {
            std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub(crate) fn read_at(path: &Path, self_pid: u32, alive: impl Fn(u32) -> bool) -> RouteRecord {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return RouteRecord::Absent,
        Err(e) => {
            return RouteRecord::Unreadable {
                path: path.to_path_buf(),
                error: e.to_string(),
            }
        }
    };
    match serde_json::from_slice::<PublishedRoute>(&bytes) {
        Ok(route) if route.pid == self_pid => RouteRecord::Mine(route),
        Ok(route) if alive(route.pid) => RouteRecord::Other(route),
        Ok(route) => RouteRecord::Stale(route),
        Err(e) => RouteRecord::Unreadable {
            path: path.to_path_buf(),
            error: e.to_string(),
        },
    }
}

/// The relay THIS goosed owns, beside the record it published. The relay's listener lives
/// exactly as long as this slot holds it.
struct OwnedRoute {
    route: PublishedRoute,
    _relay: InferenceRelay,
}

static OWNED: StdMutex<Option<OwnedRoute>> = StdMutex::new(None);

/// Install THIS goosed's route: keep the relay alive and publish the record for every window.
pub fn install(relay: InferenceRelay, route: PublishedRoute) -> Result<()> {
    publish_at(&record_path(), &route)?;
    *OWNED.lock().unwrap_or_else(|e| e.into_inner()) = Some(OwnedRoute {
        route,
        _relay: relay,
    });
    Ok(())
}

/// Drop THIS goosed's route: withdraw the record and stop the relay. Returns the route it held.
pub fn uninstall() -> Result<Option<PublishedRoute>> {
    let owned = OWNED.lock().unwrap_or_else(|e| e.into_inner()).take();
    withdraw_at(&record_path(), std::process::id())?;
    Ok(owned.map(|owned| owned.route))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(pid: u32) -> PublishedRoute {
        PublishedRoute {
            pid,
            base_url: "http://127.0.0.1:61001/relay/ab".to_string(),
            peer: "worksmacstudio-lan-9c1e2a".to_string(),
            peer_hostname: "worksmacstudio-lan-9c1e2a".to_string(),
            model_id: "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string(),
            served_model_id: "mihai-qwen3.8-27b".to_string(),
            capacity: 8,
            template_kwargs: None,
        }
    }

    #[test]
    fn the_record_names_its_owner_and_only_the_owner_withdraws_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join(RECORD_FILE);
        assert_eq!(read_at(&path, 10, |_| true), RouteRecord::Absent);

        publish_at(&path, &route(10)).unwrap();
        assert_eq!(read_at(&path, 10, |_| true), RouteRecord::Mine(route(10)));
        assert_eq!(read_at(&path, 20, |_| true), RouteRecord::Other(route(10)));
        assert_eq!(read_at(&path, 20, |_| false), RouteRecord::Stale(route(10)));
        assert!(
            read_at(&path, 20, |_| false).live().is_none(),
            "a dead owner routes nothing"
        );

        assert!(!withdraw_at(&path, 20).unwrap());
        assert!(withdraw_at(&path, 10).unwrap());
        assert_eq!(read_at(&path, 10, |_| true), RouteRecord::Absent);
    }

    #[cfg(unix)]
    #[test]
    fn the_record_carrying_the_capability_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RECORD_FILE);
        publish_at(&path, &route(10)).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_torn_record_is_unreadable_never_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RECORD_FILE);
        std::fs::write(&path, b"{\"pid\": 1, \"base_").unwrap();
        assert!(matches!(
            read_at(&path, 10, |_| true),
            RouteRecord::Unreadable { .. }
        ));
    }
}
