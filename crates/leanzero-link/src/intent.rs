//! The user's persisted mesh INTENT — whether this node should be on the mesh.
//!
//! The identity file answers "who is signed in"; it cannot answer "did the user want the
//! mesh up". Without this record every relaunch of goosed came back `LoggedIn` with the
//! daemon down (measured 2026-09-24 on both Macs after installing 3.0.23: "Mesh: not
//! connected", no goose-owned tailscaled, the placement planner blind to the peer), and a
//! reconnect-on-launch built from the identity alone would override a deliberate
//! Disconnect. So the intent is its own record, written ONLY by the user's own actions:
//! Connect writes `connected`; Disconnect and Log out write `disconnected`. A daemon
//! fault, an app quit or a relaunch never write it — they are not the user's choice.
//!
//! It lives beside the identity (`~/.leanzero/link-intent.json`), `0600`, written
//! atomically. A malformed record is a LOUD typed error naming the path — never read as
//! either value, since each wrong guess is a real harm (a node that silently stays off,
//! or one that joins the mesh after the user turned it off).
//!
//! An ABSENT record (every install that predates it) is derived from facts on disk, never
//! defaulted: a stored identity AND a goose-owned `tailscaled.state` mean this node was
//! connected under that identity before (the state file is written only by the daemon a
//! connect started, and Log out always cleared the identity) — that derivation is
//! persisted as [`IntentCause::Migrated`] so it happens once. Anything else is
//! `disconnected` with [`IntentCause::NoRecord`] — not persisted, it is the truth that no
//! connect ever happened here.

use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The file name beside the identity file.
pub const INTENT_FILE_NAME: &str = "link-intent.json";

/// Whether the user wants this node on the mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkIntent {
    /// Reconnect at launch (and whenever goosed restarts) without asking.
    Connected,
    /// Stay off until the user connects again.
    Disconnected,
}

/// Which action produced the intent — the record says why, so a reader never has to
/// guess whether a `disconnected` came from the user or from nothing having happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IntentCause {
    /// The user pressed Connect (or Retry).
    UserConnect,
    /// The user pressed Disconnect.
    UserDisconnect,
    /// The user logged out.
    UserLogout,
    /// No record existed; derived from a stored identity + the goose-owned
    /// `tailscaled.state` a previous connect left (see the module docs).
    Migrated,
    /// No record existed and nothing on disk shows a previous connect. Never persisted.
    NoRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntentRecord {
    pub intent: LinkIntent,
    pub cause: IntentCause,
    pub updated_at: DateTime<Utc>,
}

impl IntentRecord {
    pub fn new(intent: LinkIntent, cause: IntentCause) -> Self {
        Self {
            intent,
            cause,
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Error)]
pub enum IntentError {
    #[error("cannot {op} the Link intent file '{}': {source}", path.display())]
    Io {
        op: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    #[error(
        "the Link intent file '{}' is malformed ({source}); refusing to guess whether this \
         node should be on the mesh — Connect or Disconnect rewrites it",
        path.display()
    )]
    Malformed {
        path: PathBuf,
        source: serde_json::Error,
    },
}

/// Reads and writes the persisted [`IntentRecord`]. Holds only a path.
#[derive(Debug, Clone)]
pub struct IntentStore {
    path: PathBuf,
}

impl IntentStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The store beside `identity_path` ([`INTENT_FILE_NAME`] in the same directory).
    pub fn beside(identity_path: &Path) -> Self {
        let dir = identity_path.parent().unwrap_or_else(|| Path::new("."));
        Self::new(dir.join(INTENT_FILE_NAME))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The recorded intent: `Ok(None)` when no record exists, a loud
    /// [`IntentError::Malformed`] when one exists and does not parse.
    pub fn load(&self) -> Result<Option<IntentRecord>, IntentError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|source| IntentError::Malformed {
                    path: self.path.clone(),
                    source,
                }),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(IntentError::Io {
                op: "read",
                path: self.path.clone(),
                source,
            }),
        }
    }

    /// Persist atomically (temp file + rename), `0600`.
    pub fn save(&self, record: &IntentRecord) -> Result<(), IntentError> {
        let dir = self.path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(dir).map_err(|source| IntentError::Io {
            op: "create the parent directory of",
            path: self.path.clone(),
            source,
        })?;
        let bytes = serde_json::to_vec_pretty(record).expect("IntentRecord is a plain struct");
        crate::identity::write_private_atomic(&self.path, &bytes).map_err(|(op, source)| {
            IntentError::Io {
                op,
                path: self.path.clone(),
                source,
            }
        })
    }

    /// The intent this node boots with. A record on disk wins. With none: a stored
    /// identity plus the goose-owned `mesh_state_file` derive `connected`
    /// ([`IntentCause::Migrated`], persisted so the derivation runs once); otherwise
    /// `disconnected` ([`IntentCause::NoRecord`], not persisted).
    pub fn resolve(
        &self,
        identity_present: bool,
        mesh_state_file: &Path,
    ) -> Result<IntentRecord, IntentError> {
        if let Some(record) = self.load()? {
            return Ok(record);
        }
        let previously_connected = identity_present
            && mesh_state_file
                .try_exists()
                .map_err(|source| IntentError::Io {
                    op: "probe the goose-owned tailscaled state beside",
                    path: self.path.clone(),
                    source,
                })?;
        if !previously_connected {
            return Ok(IntentRecord::new(
                LinkIntent::Disconnected,
                IntentCause::NoRecord,
            ));
        }
        let record = IntentRecord::new(LinkIntent::Connected, IntentCause::Migrated);
        self.save(&record)?;
        tracing::info!(
            path = %self.path.display(),
            state_file = %mesh_state_file.display(),
            "link_intent_migrated: no intent record; a stored identity and the goose-owned \
             tailscaled state show a previous connect — intent recorded as connected"
        );
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &Path) -> IntentStore {
        IntentStore::beside(&dir.join("identity.json"))
    }

    #[test]
    fn a_saved_record_round_trips_beside_the_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        assert_eq!(store.path(), tmp.path().join(INTENT_FILE_NAME));
        let record = IntentRecord::new(LinkIntent::Disconnected, IntentCause::UserDisconnect);
        store.save(&record).unwrap();
        assert_eq!(store.load().unwrap(), Some(record));
    }

    #[cfg(unix)]
    #[test]
    fn the_record_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        store
            .save(&IntentRecord::new(
                LinkIntent::Connected,
                IntentCause::UserConnect,
            ))
            .unwrap();
        let mode = std::fs::metadata(store.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn a_malformed_record_is_loud_and_names_the_path() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        std::fs::write(store.path(), b"{\"intent\":\"sideways\"}").unwrap();
        let err = store.load().unwrap_err();
        assert!(matches!(err, IntentError::Malformed { .. }));
        assert!(err
            .to_string()
            .contains(&store.path().display().to_string()));
        // `resolve` never papers over it with a derivation.
        assert!(store
            .resolve(true, &tmp.path().join("tailscaled.state"))
            .is_err());
    }

    #[test]
    fn a_recorded_intent_wins_over_the_derivation() {
        let tmp = tempfile::tempdir().unwrap();
        let state_file = tmp.path().join("tailscaled.state");
        std::fs::write(&state_file, b"{}").unwrap();
        let store = store(tmp.path());
        store
            .save(&IntentRecord::new(
                LinkIntent::Disconnected,
                IntentCause::UserDisconnect,
            ))
            .unwrap();
        let resolved = store.resolve(true, &state_file).unwrap();
        assert_eq!(resolved.intent, LinkIntent::Disconnected);
        assert_eq!(resolved.cause, IntentCause::UserDisconnect);
    }

    #[test]
    fn no_record_with_identity_and_state_file_migrates_to_connected_once() {
        let tmp = tempfile::tempdir().unwrap();
        let state_file = tmp.path().join("tailscaled.state");
        std::fs::write(&state_file, b"{}").unwrap();
        let store = store(tmp.path());
        let resolved = store.resolve(true, &state_file).unwrap();
        assert_eq!(resolved.intent, LinkIntent::Connected);
        assert_eq!(resolved.cause, IntentCause::Migrated);
        assert_eq!(
            store.load().unwrap().map(|r| r.cause),
            Some(IntentCause::Migrated),
            "the derivation is persisted"
        );
    }

    #[test]
    fn no_record_without_both_facts_is_disconnected_and_not_persisted() {
        let tmp = tempfile::tempdir().unwrap();
        let state_file = tmp.path().join("tailscaled.state");
        let store = store(tmp.path());

        // Identity, no state file: signed in but never connected here.
        let resolved = store.resolve(true, &state_file).unwrap();
        assert_eq!(
            (resolved.intent, resolved.cause),
            (LinkIntent::Disconnected, IntentCause::NoRecord)
        );

        // State file, no identity: a previous account's leftovers — not this sign-in.
        std::fs::write(&state_file, b"{}").unwrap();
        let resolved = store.resolve(false, &state_file).unwrap();
        assert_eq!(
            (resolved.intent, resolved.cause),
            (LinkIntent::Disconnected, IntentCause::NoRecord)
        );
        assert_eq!(store.load().unwrap(), None, "NoRecord is never written");
    }
}
