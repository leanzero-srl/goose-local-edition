//! Passwordless account identity persistence.
//!
//! The [`Identity`] is the credential the desktop stores after an email-OTP sign-in:
//! the account email and a long-lived (180-day) JWT minted by the LeanZero Link worker.
//! This crate never verifies the JWT — the worker is the only verifier (see the crate
//! isolation invariant); it only persists and presents it.
//!
//! On disk it lives at `~/.leanzero/identity.json` (the path is overridable for tests,
//! and a leading `~` is expanded). The token is a 180-day credential, so the file is
//! written `0600` under a `0700` directory, and every write is atomic (temp file +
//! rename) so a crash mid-write never leaves a half-written credential. A malformed file
//! is a LOUD typed error naming the path — never silently treated as logged-out, which
//! would strand a real (if corrupt) credential and silently sign the user out.

use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub email: String,
    pub token: String,
    pub updated_at: DateTime<Utc>,
}

impl Identity {
    /// A freshly minted identity stamped `updated_at = now`.
    pub fn new(email: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            token: token.into(),
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("cannot resolve a home directory to expand '~' in the identity path")]
    NoHomeDir,
    #[error("cannot {op} identity file '{}': {source}", path.display())]
    Io {
        op: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    #[error(
        "identity file '{}' is malformed ({source}); refusing to treat a corrupt \
         credential as logged-out — move or delete it to reset",
        path.display()
    )]
    Malformed {
        path: PathBuf,
        source: serde_json::Error,
    },
}

/// The default on-disk location: `~/.leanzero/identity.json`.
pub fn default_identity_path() -> Result<PathBuf, IdentityError> {
    let home = dirs::home_dir().ok_or(IdentityError::NoHomeDir)?;
    Ok(home.join(".leanzero").join("identity.json"))
}

/// Reads and writes the persisted [`Identity`]. Cheap to clone; holds only a path.
#[derive(Debug, Clone)]
pub struct IdentityStore {
    path: PathBuf,
}

impl IdentityStore {
    /// A store over an explicit path. A leading `~` is expanded lazily at each call.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// A store over [`default_identity_path`].
    pub fn at_default() -> Result<Self, IdentityError> {
        Ok(Self::new(default_identity_path()?))
    }

    /// The resolved absolute path (tilde expanded).
    pub fn path(&self) -> Result<PathBuf, IdentityError> {
        expand_tilde(&self.path)
    }

    /// Load the identity. An absent file is `Ok(None)` (a logged-out state, not an
    /// error); a malformed file is a loud [`IdentityError::Malformed`].
    pub fn load(&self) -> Result<Option<Identity>, IdentityError> {
        let path = self.path()?;
        match std::fs::read(&path) {
            Ok(bytes) => {
                let identity =
                    serde_json::from_slice(&bytes).map_err(|source| IdentityError::Malformed {
                        path: path.clone(),
                        source,
                    })?;
                Ok(Some(identity))
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(IdentityError::Io {
                op: "read",
                path,
                source,
            }),
        }
    }

    /// Persist the identity atomically (temp file + rename), `0600` under a `0700`
    /// directory on unix.
    pub fn save(&self, identity: &Identity) -> Result<(), IdentityError> {
        let path = self.path()?;
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(dir).map_err(|source| IdentityError::Io {
            op: "create the parent directory of",
            path: path.clone(),
            source,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
        let bytes =
            serde_json::to_vec_pretty(identity).expect("Identity is a plain struct; serialize");
        write_atomic(&path, &bytes)
    }

    /// Remove the identity file. An already-absent file is `Ok(())`.
    pub fn clear(&self) -> Result<(), IdentityError> {
        let path = self.path()?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(IdentityError::Io {
                op: "remove",
                path,
                source,
            }),
        }
    }
}

fn expand_tilde(path: &Path) -> Result<PathBuf, IdentityError> {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix("~/") {
        let home = dirs::home_dir().ok_or(IdentityError::NoHomeDir)?;
        Ok(home.join(rest))
    } else if text == "~" {
        dirs::home_dir().ok_or(IdentityError::NoHomeDir)
    } else {
        Ok(path.to_path_buf())
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), IdentityError> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let base = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "identity.json".to_string());
    let tmp = dir.join(format!(".{base}.tmp.{}", std::process::id()));

    let write_result = (|| -> io::Result<()> {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&tmp)?;
        use std::io::Write;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    if let Err(source) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(IdentityError::Io {
            op: "write a temp file for",
            path: path.to_path_buf(),
            source,
        });
    }

    // OpenOptions::mode only applies on creation; force 0600 in case the temp existed.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(source) = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
        {
            let _ = std::fs::remove_file(&tmp);
            return Err(IdentityError::Io {
                op: "set 0600 on the temp file for",
                path: path.to_path_buf(),
                source,
            });
        }
    }

    std::fs::rename(&tmp, path).map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        IdentityError::Io {
            op: "atomically rename into place",
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_in(dir: &Path) -> IdentityStore {
        IdentityStore::new(dir.join("identity.json"))
    }

    #[test]
    fn absent_file_is_none_not_error() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store_in(tmp.path());
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn round_trips_through_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store_in(tmp.path());
        let identity = Identity::new("user@example.com", "jwt-token-value");
        store.save(&identity).unwrap();
        let loaded = store.load().unwrap().expect("identity present after save");
        assert_eq!(loaded, identity);
    }

    #[test]
    fn malformed_file_is_a_loud_error_naming_the_path() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("identity.json");
        std::fs::write(&path, b"{ this is not json").unwrap();
        let store = IdentityStore::new(&path);
        let err = store
            .load()
            .expect_err("malformed file must be a loud error");
        assert!(matches!(err, IdentityError::Malformed { .. }));
        assert!(
            err.to_string().contains(&path.display().to_string()),
            "error names the offending path: {err}"
        );
    }

    #[test]
    fn overwrite_is_atomic_and_leaves_no_temp() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store_in(tmp.path());
        store
            .save(&Identity::new("a@example.com", "token-a"))
            .unwrap();
        store
            .save(&Identity::new("b@example.com", "token-b"))
            .unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.email, "b@example.com");
        assert_eq!(loaded.token, "token-b");

        let leftovers: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp."))
            .collect();
        assert!(
            leftovers.is_empty(),
            "atomic write left temp files: {leftovers:?}"
        );
    }

    #[test]
    fn clear_removes_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store_in(tmp.path());
        store
            .save(&Identity::new("a@example.com", "token"))
            .unwrap();
        assert!(store.load().unwrap().is_some());
        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
        store.clear().unwrap(); // absent file is Ok
    }

    #[cfg(unix)]
    #[test]
    fn token_file_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let store = store_in(tmp.path());
        store
            .save(&Identity::new("a@example.com", "secret"))
            .unwrap();
        let mode = std::fs::metadata(store.path().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode, 0o600,
            "identity file must be private (0600), got {mode:o}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn tilde_is_expanded() {
        let home = dirs::home_dir().unwrap();
        let store = IdentityStore::new("~/.leanzero/identity.json");
        assert_eq!(
            store.path().unwrap(),
            home.join(".leanzero").join("identity.json")
        );
    }
}
