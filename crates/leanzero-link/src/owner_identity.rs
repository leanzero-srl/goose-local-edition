//! Immutable host identity, independent of account credentials and display names.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerIdentity {
    version: u32,
    pub owner_node_id: Uuid,
    pub registration_id: Uuid,
}

#[derive(Debug, Error)]
pub enum OwnerIdentityError {
    #[error("host identity I/O at '{}': {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("host identity data at '{}' is invalid: {source}", path.display())]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("host identity at '{}' cannot be used: {reason}", path.display())]
    Invalid { path: PathBuf, reason: String },
}

impl OwnerIdentity {
    pub fn load_or_create(path: impl AsRef<Path>) -> Result<Self, OwnerIdentityError> {
        let path = path.as_ref();
        let bytes = load_or_create_private(path, || {
            let identity = Self {
                version: 1,
                owner_node_id: Uuid::new_v4(),
                registration_id: Uuid::new_v4(),
            };
            serde_json::to_vec(&identity).map_err(|source| OwnerIdentityError::Json {
                path: path.to_path_buf(),
                source,
            })
        })?;
        let identity: Self =
            serde_json::from_slice(&bytes).map_err(|source| OwnerIdentityError::Json {
                path: path.to_path_buf(),
                source,
            })?;
        if identity.version != 1
            || identity.owner_node_id.get_version_num() != 4
            || identity.registration_id.get_version_num() != 4
            || identity.owner_node_id == identity.registration_id
        {
            return Err(invalid(path, "unsupported version or invalid identifiers"));
        }
        Ok(identity)
    }
}

pub(crate) fn invalid(path: &Path, reason: impl Into<String>) -> OwnerIdentityError {
    OwnerIdentityError::Invalid {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

fn io_error(path: &Path, source: io::Error) -> OwnerIdentityError {
    OwnerIdentityError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn read_private(path: &Path) -> Result<Option<Vec<u8>>, OwnerIdentityError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(path, error)),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid(path, "expected a regular private file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(invalid(
                path,
                "file must not be accessible by group or others",
            ));
        }
    }
    fs::read(path)
        .map(Some)
        .map_err(|error| io_error(path, error))
}

/// Publish a completely synced file without replacing another process's identity.
/// A rename would overwrite the winner during simultaneous first launches.
pub fn load_or_create_private(
    path: &Path,
    create: impl FnOnce() -> Result<Vec<u8>, OwnerIdentityError>,
) -> Result<Vec<u8>, OwnerIdentityError> {
    if let Some(bytes) = read_private(path)? {
        return Ok(bytes);
    }
    let directory = path
        .parent()
        .filter(|directory| !directory.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(directory).map_err(|error| io_error(directory, error))?;
    let bytes = create()?;
    let temporary = directory.join(format!(".controller-identity-{}.tmp", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| io_error(&temporary, error))?;
    let pending = PendingFile(temporary);
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| io_error(&pending.0, error))?;
    match fs::hard_link(&pending.0, path) {
        Ok(()) => {
            #[cfg(unix)]
            fs::File::open(directory)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| io_error(directory, error))?;
            Ok(bytes)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => read_private(path)?
            .ok_or_else(|| invalid(path, "concurrent identity disappeared during creation")),
        Err(error) => Err(io_error(path, error)),
    }
}

struct PendingFile(PathBuf);

impl Drop for PendingFile {
    fn drop(&mut self) {
        // The published identity is already durable. A failed temp unlink cannot
        // justify deleting or replacing it; report the orphan for later cleanup.
        if let Err(error) = fs::remove_file(&self.0) {
            tracing::warn!(path = %self.0.display(), %error, "controller identity temp cleanup failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_first_launches_share_one_persisted_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("owner.json");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(12));
        let threads: Vec<_> = (0..12)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    OwnerIdentity::load_or_create(path).unwrap()
                })
            })
            .collect();
        let identities: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert!(identities.iter().all(|identity| identity == &identities[0]));
        assert_eq!(OwnerIdentity::load_or_create(&path).unwrap(), identities[0]);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn corrupt_identity_is_never_silently_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("owner.json");
        OwnerIdentity::load_or_create(&path).unwrap();
        fs::write(&path, b"not json").unwrap();
        assert!(matches!(
            OwnerIdentity::load_or_create(&path),
            Err(OwnerIdentityError::Json { .. })
        ));
        assert_eq!(fs::read(&path).unwrap(), b"not json");
    }

    #[cfg(unix)]
    #[test]
    fn private_file_and_no_symlink_following() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("owner.json");
        OwnerIdentity::load_or_create(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let alias = directory.path().join("alias.json");
        symlink(&path, &alias).unwrap();
        assert!(OwnerIdentity::load_or_create(alias).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(OwnerIdentity::load_or_create(path).is_err());
    }
}
