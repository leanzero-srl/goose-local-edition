//! What this Mac's owner last started serving MLX chat from — and did not stop — so the next
//! launch can bring it back.
//!
//! Every app update or relaunch stops goosed, and goosed stops what it supervises on its way out
//! (`shutdown.rs`): the engine, the relay to a peer, the split's ranks. None of those exits is the
//! owner's choice, so none of them touches this record. It is written when a start the owner made
//! from THIS Mac is accepted (the single engine's Mount, a remote single's start, the split's
//! start) and removed only by the matching explicit stop (Unmount, the remote Stop, the split's
//! Stop). A peer's Mount reaching this Mac over LeanZero Link is that peer's start, not this
//! owner's: the mesh proxy calls the engine cores directly and never writes here.
//!
//! The desktop reads it at launch (`mlxEngine/servingIntent`) and restores it through the same
//! start paths a click takes, so the memory gate, Make room and the Link reconnect all apply.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::paths::Paths;

const RECORD_FILE: &str = "mlx-serving-intent.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ServingIntent {
    /// This Mac's single engine, serving `model_id`.
    #[serde(rename_all = "camelCase")]
    Single { model_id: String },
    /// The single engine on Link peer `peer` (a node id), this Mac's chat routed to it.
    /// `peer_name` is what the owner calls that Mac, for the line shown while restoring.
    #[serde(rename_all = "camelCase")]
    RemoteSingle {
        peer: String,
        peer_name: String,
        model_id: String,
    },
    /// The split across the Macs of the saved distributed config, serving `model_id`.
    #[serde(rename_all = "camelCase")]
    Split { model_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentKind {
    Single,
    RemoteSingle,
    Split,
}

impl ServingIntent {
    pub fn kind(&self) -> IntentKind {
        match self {
            Self::Single { .. } => IntentKind::Single,
            Self::RemoteSingle { .. } => IntentKind::RemoteSingle,
            Self::Split { .. } => IntentKind::Split,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IntentRecord {
    Absent,
    Present(ServingIntent),
    Unreadable { path: PathBuf, error: String },
}

pub fn record_path() -> PathBuf {
    Paths::in_state_dir(RECORD_FILE)
}

pub fn read() -> IntentRecord {
    read_at(&record_path())
}

/// The owner started `intent`: it is what the next launch brings back.
pub fn remember(intent: &ServingIntent) -> Result<()> {
    remember_at(&record_path(), intent)
}

/// The owner stopped what serves as `kind`: nothing of that kind comes back. A record of another
/// kind stays — stopping a leftover local engine must not forget a route to a peer. An unreadable
/// record is removed: it can restore nothing, and it would name its failure at every launch.
pub fn forget(kind: IntentKind) -> Result<bool> {
    forget_at(&record_path(), kind)
}

/// Where a model was deleted: on this Mac's models dir, or on Link peer `node` (its node id).
/// `in_split`: the saved split names that peer as one of its Macs (`link:<node>`), so the split
/// loads the peer's copy too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletedOn<'a> {
    ThisMac,
    Peer { node: &'a str, in_split: bool },
}

/// The owner deleted `model_id`: a record that would bring that copy back is forgotten. This
/// Mac's copy serves the single engine and the split; a peer's copy serves the remote single
/// routed to that peer, and the split when the split runs a rank there. Any other record stays.
/// A split peer reached by an ssh alias is not named by a Link node id, so a delete over Link
/// cannot be matched to it and its split record stays.
pub fn forget_deleted_model(model_id: &str, on: DeletedOn<'_>) -> Result<bool> {
    forget_deleted_model_at(&record_path(), model_id, on)
}

pub(crate) fn forget_deleted_model_at(
    path: &Path,
    model_id: &str,
    on: DeletedOn<'_>,
) -> Result<bool> {
    let names_it = match (read_at(path), on) {
        (
            IntentRecord::Present(
                ServingIntent::Single { model_id: m } | ServingIntent::Split { model_id: m },
            ),
            DeletedOn::ThisMac,
        ) => m == model_id,
        (
            IntentRecord::Present(ServingIntent::RemoteSingle {
                peer, model_id: m, ..
            }),
            DeletedOn::Peer { node, .. },
        ) => peer == node && m == model_id,
        (
            IntentRecord::Present(ServingIntent::Split { model_id: m }),
            DeletedOn::Peer { in_split: true, .. },
        ) => m == model_id,
        _ => false,
    };
    if names_it {
        std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
    }
    Ok(names_it)
}

/// The launch restore's read, with restore on: a single-engine record whose model folder is
/// provably gone from `models_dir` (the owner deleted it, here or by hand) can bring nothing back,
/// so it is forgotten and the launch shows nothing. Out of reach is not gone
/// (`model_folder_gone`), and a model the engine is serving (`serving`) is never forgotten.
/// Only the launch restore calls this: every other reader of the record reads it as it stands.
pub(crate) fn forget_if_model_gone_at(
    path: &Path,
    models_dir: &Path,
    serving: Option<&str>,
) -> IntentRecord {
    let record = read_at(path);
    let IntentRecord::Present(ServingIntent::Single { model_id }) = &record else {
        return record;
    };
    if serving == Some(model_id.as_str())
        || !goose_sidecar::hf::model_folder_gone(models_dir, model_id)
    {
        return record;
    }
    match forget_at(path, IntentKind::Single) {
        Ok(_) => tracing::info!(
            model_id,
            models_dir = %models_dir.display(),
            "the recorded MLX model is no longer on this Mac; its restore record was forgotten"
        ),
        Err(e) => tracing::warn!(
            model_id,
            error = %format!("{e:#}"),
            "the recorded MLX model is no longer on this Mac; its restore record could not be removed"
        ),
    }
    IntentRecord::Absent
}

pub(crate) fn read_at(path: &Path) -> IntentRecord {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return IntentRecord::Absent,
        Err(e) => {
            return IntentRecord::Unreadable {
                path: path.to_path_buf(),
                error: e.to_string(),
            }
        }
    };
    match serde_json::from_slice::<ServingIntent>(&bytes) {
        Ok(intent) => IntentRecord::Present(intent),
        Err(e) => IntentRecord::Unreadable {
            path: path.to_path_buf(),
            error: e.to_string(),
        },
    }
}

pub(crate) fn remember_at(path: &Path, intent: &ServingIntent) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension(format!("json.{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(intent)?)
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("renaming onto {}", path.display()))
}

pub(crate) fn forget_at(path: &Path, kind: IntentKind) -> Result<bool> {
    let remove = match read_at(path) {
        IntentRecord::Absent => false,
        IntentRecord::Present(intent) => intent.kind() == kind,
        IntentRecord::Unreadable { .. } => true,
    };
    if remove {
        std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
    }
    Ok(remove)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote() -> ServingIntent {
        ServingIntent::RemoteSingle {
            peer: "worksmacstudio-lan-9c1e2a".to_string(),
            peer_name: "Work's Mac Studio".to_string(),
            model_id: "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string(),
        }
    }

    #[test]
    fn a_started_intent_survives_until_its_own_kind_is_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join(RECORD_FILE);
        assert_eq!(read_at(&path), IntentRecord::Absent);

        remember_at(&path, &remote()).unwrap();
        assert_eq!(read_at(&path), IntentRecord::Present(remote()));

        // Unmounting a leftover local engine is not stopping the route to the peer.
        assert!(!forget_at(&path, IntentKind::Single).unwrap());
        assert_eq!(read_at(&path), IntentRecord::Present(remote()));

        assert!(forget_at(&path, IntentKind::RemoteSingle).unwrap());
        assert_eq!(read_at(&path), IntentRecord::Absent);
        assert!(!forget_at(&path, IntentKind::RemoteSingle).unwrap());
    }

    #[test]
    fn the_latest_start_is_what_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RECORD_FILE);
        remember_at(&path, &remote()).unwrap();
        let split = ServingIntent::Split {
            model_id: "mlx-community/Qwen3.5-397B-A17B-4bit".to_string(),
        };
        remember_at(&path, &split).unwrap();
        assert_eq!(read_at(&path), IntentRecord::Present(split));
    }

    #[test]
    fn the_wire_shape_is_tagged_camel_case() {
        assert_eq!(
            serde_json::to_value(remote()).unwrap(),
            serde_json::json!({
                "kind": "remoteSingle",
                "peer": "worksmacstudio-lan-9c1e2a",
                "peerName": "Work's Mac Studio",
                "modelId": "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx",
            })
        );
        assert_eq!(
            serde_json::to_value(ServingIntent::Single {
                model_id: "m".to_string()
            })
            .unwrap(),
            serde_json::json!({"kind": "single", "modelId": "m"})
        );
    }

    fn single(model_id: &str) -> ServingIntent {
        ServingIntent::Single {
            model_id: model_id.to_string(),
        }
    }

    fn models_dir_with(root: &Path, ids: &[&str]) -> PathBuf {
        let models = root.join("models");
        for id in ids {
            std::fs::create_dir_all(models.join(id)).unwrap();
            std::fs::write(models.join(id).join("config.json"), b"{}").unwrap();
        }
        models
    }

    #[test]
    fn a_record_of_a_model_gone_from_this_mac_is_forgotten_at_launch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RECORD_FILE);
        let models = models_dir_with(dir.path(), &["pub/kept"]);

        remember_at(&path, &single("pub/deleted")).unwrap();
        assert_eq!(
            forget_if_model_gone_at(&path, &models, None),
            IntentRecord::Absent
        );
        assert_eq!(
            read_at(&path),
            IntentRecord::Absent,
            "the record itself is gone"
        );

        remember_at(&path, &single("pub/kept")).unwrap();
        assert_eq!(
            forget_if_model_gone_at(&path, &models, None),
            IntentRecord::Present(single("pub/kept"))
        );
        assert_eq!(read_at(&path), IntentRecord::Present(single("pub/kept")));
    }

    #[test]
    fn an_unproven_absence_keeps_the_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RECORD_FILE);
        let empty = dir.path().join("models");
        std::fs::create_dir_all(&empty).unwrap();

        remember_at(&path, &single("pub/m")).unwrap();
        assert_eq!(
            forget_if_model_gone_at(&path, &empty, None),
            IntentRecord::Present(single("pub/m")),
            "an empty models dir may be an unmounted share"
        );
        remember_at(&path, &remote()).unwrap();
        let models = models_dir_with(dir.path(), &["pub/other"]);
        assert_eq!(
            forget_if_model_gone_at(&path, &models, None),
            IntentRecord::Present(remote()),
            "a remote single's model lives on the peer"
        );
    }

    #[test]
    fn deleting_a_model_forgets_only_a_record_that_names_that_copy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RECORD_FILE);
        let model = "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx";

        remember_at(&path, &single(model)).unwrap();
        assert!(!forget_deleted_model_at(&path, "pub/other", DeletedOn::ThisMac).unwrap());
        assert_eq!(read_at(&path), IntentRecord::Present(single(model)));
        assert!(forget_deleted_model_at(&path, model, DeletedOn::ThisMac).unwrap());
        assert_eq!(read_at(&path), IntentRecord::Absent);

        let split = ServingIntent::Split {
            model_id: model.to_string(),
        };
        remember_at(&path, &split).unwrap();
        assert!(forget_deleted_model_at(&path, model, DeletedOn::ThisMac).unwrap());
        assert_eq!(read_at(&path), IntentRecord::Absent);

        remember_at(&path, &remote()).unwrap();
        assert!(
            !forget_deleted_model_at(&path, model, DeletedOn::ThisMac).unwrap(),
            "this Mac's copy is not the peer's"
        );
        assert!(!forget_deleted_model_at(
            &path,
            model,
            DeletedOn::Peer {
                node: "another-mac",
                in_split: false
            }
        )
        .unwrap());
        assert_eq!(read_at(&path), IntentRecord::Present(remote()));
        assert!(forget_deleted_model_at(
            &path,
            model,
            DeletedOn::Peer {
                node: "worksmacstudio-lan-9c1e2a",
                in_split: false,
            }
        )
        .unwrap());
        assert_eq!(read_at(&path), IntentRecord::Absent);

        assert!(!forget_deleted_model_at(&path, model, DeletedOn::ThisMac).unwrap());
    }

    #[test]
    fn deleting_the_peer_copy_a_split_loads_forgets_the_split() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RECORD_FILE);
        let split = ServingIntent::Split {
            model_id: "pub/flash".to_string(),
        };
        remember_at(&path, &split).unwrap();
        let peer = |in_split| DeletedOn::Peer {
            node: "studio",
            in_split,
        };
        assert!(!forget_deleted_model_at(&path, "pub/flash", peer(false)).unwrap());
        assert!(!forget_deleted_model_at(&path, "pub/other", peer(true)).unwrap());
        assert_eq!(read_at(&path), IntentRecord::Present(split));
        assert!(forget_deleted_model_at(&path, "pub/flash", peer(true)).unwrap());
        assert_eq!(read_at(&path), IntentRecord::Absent);
    }

    #[test]
    fn a_model_the_engine_serves_is_never_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RECORD_FILE);
        let models = models_dir_with(dir.path(), &["pub/kept"]);
        remember_at(&path, &single("pub/gone-from-disk")).unwrap();
        assert_eq!(
            forget_if_model_gone_at(&path, &models, Some("pub/gone-from-disk")),
            IntentRecord::Present(single("pub/gone-from-disk"))
        );
        assert_eq!(
            read_at(&path),
            IntentRecord::Present(single("pub/gone-from-disk"))
        );
    }

    #[test]
    fn a_models_dir_holding_only_ds_store_forgets_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RECORD_FILE);
        let models = dir.path().join("models");
        std::fs::create_dir_all(models.join(".locks")).unwrap();
        std::fs::write(models.join(".DS_Store"), b"x").unwrap();
        remember_at(&path, &single("pub/m")).unwrap();
        assert_eq!(
            forget_if_model_gone_at(&path, &models, None),
            IntentRecord::Present(single("pub/m"))
        );
    }

    #[test]
    fn an_unreadable_record_is_named_and_a_stop_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RECORD_FILE);
        std::fs::write(&path, b"{\"kind\":\"teleport\"}").unwrap();
        assert!(matches!(read_at(&path), IntentRecord::Unreadable { .. }));
        assert!(forget_at(&path, IntentKind::Split).unwrap());
        assert_eq!(read_at(&path), IntentRecord::Absent);
    }
}
