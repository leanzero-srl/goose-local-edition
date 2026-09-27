use etcetera::{choose_app_strategy, AppStrategy, AppStrategyArgs};
use once_cell::sync::Lazy;
use rmcp::{ServerHandler, ServiceExt};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

// NOTE: "Block" is kept here for backwards compatibility with existing
// user config/data directories. Changing this would orphan existing installations.
pub static APP_STRATEGY: Lazy<AppStrategyArgs> = Lazy::new(|| AppStrategyArgs {
    top_level_domain: "Block".to_string(),
    author: "Block".to_string(),
    app_name: "goose".to_string(),
});

/// goose's config dir by the rule of `goose::config::paths::Paths::config_dir`, which this crate
/// cannot call (goose depends on goose-mcp): `<GOOSE_PATH_ROOT>/config` when the root is set, else
/// the app strategy's config dir. Q-184: the memory tool derived its dir from the strategy alone,
/// so under a root it wrote global memories and proposals to the owner's `~/.config/goose` while
/// recall, the importer and the desktop Memories view read `<root>/config`. goose's
/// `config::paths` tests pin this equal to `Paths::config_dir()`.
pub fn goose_config_dir() -> PathBuf {
    goose_config_dir_under(std::env::var_os("GOOSE_PATH_ROOT"))
}

/// [`goose_config_dir`] for an explicit root, so a test never changes the process env.
pub fn goose_config_dir_under(path_root: Option<OsString>) -> PathBuf {
    match path_root {
        Some(root) => PathBuf::from(root).join("config"),
        None => choose_app_strategy(APP_STRATEGY.clone())
            .expect("goose requires a home dir")
            .config_dir(),
    }
}

pub mod autovisualiser;
pub mod computercontroller;
pub mod mcp_server_runner;
mod memory;
#[cfg(target_os = "macos")]
pub mod peekaboo;
pub mod subprocess;
pub mod tutorial;

pub use autovisualiser::AutoVisualiserRouter;
pub use computercontroller::ComputerControllerServer;
pub use memory::MemoryServer;
pub use tutorial::TutorialServer;

/// Type definition for a function that spawns and serves a builtin extension server
pub type SpawnServerFn = fn(tokio::io::DuplexStream, tokio::io::DuplexStream);

/// Serve a builtin extension server on the in-process transport a [`SpawnServerFn`] receives.
/// Public so goose can register the builtin this crate cannot configure (memory, Q-187).
pub fn spawn_and_serve<S>(
    name: &'static str,
    server: S,
    transport: (tokio::io::DuplexStream, tokio::io::DuplexStream),
) where
    S: ServerHandler + Send + 'static,
{
    tokio::spawn(async move {
        match server.serve(transport).await {
            Ok(running) => {
                let _ = running.waiting().await;
            }
            Err(e) => tracing::error!(builtin = name, error = %e, "server error"),
        }
    });
}

macro_rules! builtin {
    ($name:ident, $server_ty:ty) => {{
        fn spawn(r: tokio::io::DuplexStream, w: tokio::io::DuplexStream) {
            spawn_and_serve(stringify!($name), <$server_ty>::new(), (r, w));
        }
        (stringify!($name), spawn as SpawnServerFn)
    }};
}

/// The builtins that need no goose config. `memory` is NOT here (Q-187): its `memory_proposals`
/// setting lives in goose's config, which this crate cannot read (goose depends on it), and the
/// memory builtin this map used to build ignored the setting — proposals stayed on in every
/// desktop and CLI session of a user who had turned them off. goose registers memory beside these
/// (`goose::builtin_extension::builtin_extensions`).
pub static BUILTIN_EXTENSIONS: Lazy<HashMap<&'static str, SpawnServerFn>> = Lazy::new(|| {
    HashMap::from([
        builtin!(autovisualiser, AutoVisualiserRouter),
        builtin!(computercontroller, ComputerControllerServer),
        builtin!(tutorial, TutorialServer),
    ])
});
