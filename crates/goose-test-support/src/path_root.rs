//! The state root of a goose test process.
//!
//! Every goose path — config, data, state (the MLX owner record, serving intent, remote route,
//! sessions, memory) — and LeanZero Link's `.leanzero` dir hang from `GOOSE_PATH_ROOT` when it is
//! set. A test process that leaves it unset reads and writes the owner's LIVE files: a running
//! distributed split flipped `a_stopped_engine_reports_single_mode_and_the_persisted_config`
//! (2026-09-25), and a test that writes there can corrupt the product's state.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub const PATH_ROOT_ENV: &str = "GOOSE_PATH_ROOT";
const DISABLE_KEYRING_ENV: &str = "GOOSE_DISABLE_KEYRING";

/// This process's root: the one the caller exported, else a fresh dir under the system temp dir,
/// exported so every crate in the process resolves under it. With a fresh root the keyring is
/// disabled too, or secrets would escape the root into the owner's system keychain. Call it from a
/// `#[ctor::ctor]`, before any test thread reads the environment.
pub fn hermetic_path_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        if let Some(root) = std::env::var_os(PATH_ROOT_ENV) {
            return PathBuf::from(root);
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("goose-path-root-{}-{nanos}", std::process::id()));
        std::fs::create_dir(&root)
            .unwrap_or_else(|e| panic!("creating the test path root {}: {e}", root.display()));
        std::env::set_var(PATH_ROOT_ENV, &root);
        if std::env::var_os(DISABLE_KEYRING_ENV).is_none() {
            std::env::set_var(DISABLE_KEYRING_ENV, "1");
        }
        root
    })
}
