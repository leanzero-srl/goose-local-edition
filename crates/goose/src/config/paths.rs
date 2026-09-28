use etcetera::{choose_app_strategy, AppStrategy, AppStrategyArgs};
use std::path::PathBuf;

pub struct Paths;

#[cfg(test)]
#[ctor::ctor]
fn hermetic_path_root_for_unit_tests() {
    goose_test_support::hermetic_path_root();
}

impl Paths {
    /// `GOOSE_PATH_ROOT` when set: every goose dir then hangs from it instead of the owner's home.
    pub fn root_override() -> Option<PathBuf> {
        let exported = std::env::var_os("GOOSE_PATH_ROOT").map(PathBuf::from);
        // A unit test that clears the variable (env_lock, remove_var) still never reaches the
        // owner's live dirs.
        #[cfg(test)]
        let exported =
            exported.or_else(|| Some(goose_test_support::hermetic_path_root().to_path_buf()));
        exported
    }

    fn get_dir(dir_type: DirType) -> PathBuf {
        if let Some(base) = Self::root_override() {
            match dir_type {
                DirType::Config => base.join("config"),
                DirType::Data => base.join("data"),
                DirType::State => base.join("state"),
                DirType::Plugins => base.join(".agents").join("plugins"),
                DirType::AgentsHome => base.join(".agents"),
            }
        } else {
            // NOTE: "Block" is kept here for backwards compatibility with existing
            // user config/data directories (e.g. ~/Library/Application Support/Block/goose/).
            // Changing this would orphan existing installations.
            let strategy = choose_app_strategy(AppStrategyArgs {
                top_level_domain: "Block".to_string(),
                author: "Block".to_string(),
                app_name: "goose".to_string(),
            })
            .expect("goose requires a home dir");

            match dir_type {
                DirType::Config => strategy.config_dir(),
                DirType::Data => strategy.data_dir(),
                DirType::State => strategy.state_dir().unwrap_or(strategy.data_dir()),
                DirType::Plugins => strategy.home_dir().join(".agents").join("plugins"),
                DirType::AgentsHome => strategy.home_dir().join(".agents"),
            }
        }
    }

    pub fn config_dir() -> PathBuf {
        Self::get_dir(DirType::Config)
    }

    pub fn data_dir() -> PathBuf {
        Self::get_dir(DirType::Data)
    }

    pub fn state_dir() -> PathBuf {
        Self::get_dir(DirType::State)
    }

    pub fn plugins_dir() -> PathBuf {
        Self::get_dir(DirType::Plugins)
    }

    pub fn agents_home_dir() -> PathBuf {
        Self::get_dir(DirType::AgentsHome)
    }

    pub fn in_agents_home_dir(subpath: &str) -> PathBuf {
        Self::agents_home_dir().join(subpath)
    }

    /// The folder for work that belongs to NO session — Settings' extension test, a provider built
    /// only to list its models: the user's home folder, chosen here by name. Never the process cwd:
    /// goosed's cwd is an accident of how it was started (since Q-257 the desktop starts it in
    /// $HOME; a terminal `goose serve` in wherever the terminal was), and work for a session always
    /// runs in the session's own folder (Q-263..Q-267).
    pub fn sessionless_dir() -> anyhow::Result<PathBuf> {
        dirs::home_dir().ok_or_else(|| {
            anyhow::anyhow!(
                "no home folder is known for this user, so work outside a chat has no folder"
            )
        })
    }

    /// goose's legacy `~/.goose` (global agents and recipes predate `.agents`): the sibling of
    /// [`Self::agents_home_dir`], so `<GOOSE_PATH_ROOT>/.goose` under a root (Q-197).
    pub fn in_legacy_home_dir(subpath: &str) -> PathBuf {
        Self::agents_home_dir()
            .with_file_name(".goose")
            .join(subpath)
    }

    pub fn in_state_dir(subpath: &str) -> PathBuf {
        Self::state_dir().join(subpath)
    }

    pub fn in_config_dir(subpath: &str) -> PathBuf {
        Self::config_dir().join(subpath)
    }

    pub fn in_data_dir(subpath: &str) -> PathBuf {
        Self::data_dir().join(subpath)
    }
}

enum DirType {
    Config,
    Data,
    State,
    Plugins,
    AgentsHome,
}

#[cfg(test)]
mod tests {
    use super::Paths;

    /// Q-184: goose-mcp cannot call `Paths` (goose depends on it), so its memory tool mirrors the
    /// config-dir rule in `goose_mcp::goose_config_dir`. This pins the mirror to the rule: the memory
    /// tool's global memories and proposals are the dirs recall, the importer and the desktop read.
    #[test]
    fn the_memory_tool_writes_where_goose_reads_under_a_path_root() {
        let root = tempfile::tempdir().unwrap();
        let _guard = env_lock::lock_env([
            ("GOOSE_PATH_ROOT", root.path().to_str()),
            ("GOOSE_MEMORY_PROPOSALS", Some("true")),
        ]);
        assert_eq!(Paths::config_dir(), root.path().join("config"));
        assert_eq!(goose_mcp::goose_config_dir(), Paths::config_dir());

        for server in [
            crate::builtin_extension::memory_server(root.path().join("project")),
            goose_mcp::MemoryServer::with_proposals(true, root.path().join("project")),
        ] {
            assert_eq!(server.global_memory_dir(), Paths::in_config_dir("memory"));
            assert_eq!(
                server.proposals_dir(),
                Some(Paths::in_config_dir("proposals").as_path())
            );
        }
    }

    /// Q-197, unset root: the legacy `.goose` beside `Paths`' `.agents` is the owner's `~/.goose`,
    /// the folder agents and recipes were always read from.
    #[test]
    fn unset_the_legacy_home_is_the_old_dot_goose() {
        let agents_home = etcetera::home_dir().unwrap().join(".agents");
        assert_eq!(
            agents_home.with_file_name(".goose").join("agents"),
            dirs::home_dir().unwrap().join(".goose").join("agents")
        );
    }

    /// Q-197: under a root the legacy folder hangs from it, beside `.agents`.
    #[test]
    fn the_legacy_home_hangs_from_the_path_root() {
        let root = tempfile::tempdir().unwrap();
        let _guard = env_lock::lock_env([("GOOSE_PATH_ROOT", root.path().to_str())]);
        assert_eq!(
            Paths::in_legacy_home_dir("agents"),
            root.path().join(".goose").join("agents")
        );
        assert_eq!(
            Paths::in_agents_home_dir("agents"),
            root.path().join(".agents").join("agents")
        );
    }
}
