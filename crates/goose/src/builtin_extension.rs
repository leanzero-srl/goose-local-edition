use crate::config::Config;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::RwLock;

pub type SpawnServerFn = goose_mcp::SpawnServerFn;

static BUILTIN_REGISTRY: Lazy<RwLock<HashMap<&'static str, SpawnServerFn>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Register a builtin extension into the global registry
pub fn register_builtin_extension(name: &'static str, spawn_fn: SpawnServerFn) {
    BUILTIN_REGISTRY.write().unwrap().insert(name, spawn_fn);
}

/// Register multiple builtin extensions from a HashMap
pub fn register_builtin_extensions(extensions: HashMap<&'static str, SpawnServerFn>) {
    let mut registry = BUILTIN_REGISTRY.write().unwrap();
    registry.extend(extensions);
}

/// Get a copy of all registered builtin extensions
pub fn get_builtin_extension(name: &str) -> Option<SpawnServerFn> {
    BUILTIN_REGISTRY.read().unwrap().get(name).cloned()
}

pub fn get_builtin_extension_names() -> Vec<&'static str> {
    BUILTIN_REGISTRY.read().unwrap().keys().copied().collect()
}

/// Every builtin goose serves in-process: goose-mcp's, plus memory, which goose-mcp cannot build
/// because its `memory_proposals` setting lives in goose's config (Q-187). goosed and the CLI
/// register exactly this map.
pub fn builtin_extensions() -> HashMap<&'static str, SpawnServerFn> {
    let mut all = goose_mcp::BUILTIN_EXTENSIONS.clone();
    all.insert("memory", spawn_memory);
    all
}

/// THE memory server, for every entry point — the in-process builtin every desktop and CLI
/// session uses, `goose mcp memory` (the Docker path) and `goosed mcp memory` — so the owner's
/// `memory_proposals` setting reaches all of them. Read when the server is built, so a changed
/// setting applies from the next session. Absent = ON (`Config::memory_proposals_enabled`, the
/// default the desktop's "Ask before saving memories" switch shows). `working_dir` is the session's
/// folder: the in-process builtin gets it from the extension manager (Q-264), `goose mcp memory`
/// from its own cwd, which the manager spawns it in.
pub fn memory_server(working_dir: std::path::PathBuf) -> goose_mcp::MemoryServer {
    memory_server_for(Config::global(), working_dir)
}

fn memory_server_for(config: &Config, working_dir: std::path::PathBuf) -> goose_mcp::MemoryServer {
    goose_mcp::MemoryServer::with_proposals(config.memory_proposals_enabled(), working_dir)
}

fn spawn_memory(
    r: tokio::io::DuplexStream,
    w: tokio::io::DuplexStream,
    working_dir: std::path::PathBuf,
) {
    goose_mcp::spawn_and_serve("memory", memory_server(working_dir), (r, w));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(yaml: &str) -> (tempfile::TempDir, Config) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, yaml).unwrap();
        let config = Config::new_with_file_secrets(&path, dir.path().join("secrets.yaml")).unwrap();
        (dir, config)
    }

    /// Q-187: the memory server follows `memory_proposals` — off writes directly, on or absent files
    /// proposals. Before, the in-process builtin was `MemoryServer::new()` and filed proposals for a
    /// user who had switched them off.
    #[test]
    fn the_memory_server_honours_memory_proposals() {
        let _guard = env_lock::lock_env([("GOOSE_MEMORY_PROPOSALS", None::<&str>)]);
        for (yaml, filed) in [
            ("GOOSE_MEMORY_PROPOSALS: false\n", false),
            ("GOOSE_MEMORY_PROPOSALS: true\n", true),
            ("{}\n", true),
        ] {
            let (dir, config) = config_with(yaml);
            let server = memory_server_for(&config, dir.path().to_path_buf());
            assert_eq!(server.proposals_dir().is_some(), filed, "{yaml}");
        }
    }

    /// Q-187: `memory_server` is the entry point's door — it reads the process config (here the
    /// env override, which `Config::get_param` reads first).
    #[test]
    fn memory_server_reads_the_process_config() {
        let _guard = env_lock::lock_env([("GOOSE_MEMORY_PROPOSALS", Some("false"))]);
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            memory_server(dir.path().to_path_buf()).proposals_dir(),
            None
        );
    }

    /// Q-187: the in-process registry's memory is goose's config-reading spawn, and goose-mcp's own
    /// map carries no memory of its own that could be registered in its place.
    #[test]
    fn the_registered_memory_builtin_is_the_config_reading_one() {
        assert!(!goose_mcp::BUILTIN_EXTENSIONS.contains_key("memory"));
        let all = builtin_extensions();
        assert_eq!(all["memory"] as usize, spawn_memory as usize);
        for name in goose_mcp::BUILTIN_EXTENSIONS.keys() {
            assert!(all.contains_key(name), "{name}");
        }
    }
}
