//! The swarm config's one writer. Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases).
//!
//! Q-465: `load_config` answers a `swarm:` block that fails to deserialize with the DEFAULTS (and
//! records `CONFIG_PARSE_ERROR` for the levers echo). Every `goose swarm pool …` / `cloud …`
//! command then saved that default config plus its one change over the operator's whole block.
//! The save now re-reads the stored block and refuses while it does not deserialize.

use super::{merge_json, SwarmConfig, SWARM_CONFIG_KEY};
use anyhow::{anyhow, bail, Result};
use goose::config::{Config, ConfigError};

pub(super) fn save(cfg: &SwarmConfig) -> Result<()> {
    let config = Config::global();
    let stored = match config.get(SWARM_CONFIG_KEY, false) {
        Ok(stored) => Some(stored),
        Err(ConfigError::NotFound(_)) => None,
        Err(e) => bail!("the swarm config could not be read ({e}); nothing was saved"),
    };
    if let Err(reason) = stored_block_deserializes(stored) {
        bail!(
            "the `{SWARM_CONFIG_KEY}:` block in {} does not deserialize ({reason}); it was left \
             untouched and nothing was saved — fix the block, then retry",
            config.path()
        );
    }
    config
        .set_param(SWARM_CONFIG_KEY, cfg)
        .map_err(|e| anyhow!("failed to save swarm config: {e}"))
}

/// The same composition `merge_config_over_defaults` reads with — the stored block merged over
/// `SwarmConfig::default()` — so a block the reader accepts is exactly a block the save accepts.
fn stored_block_deserializes(stored: Option<serde_json::Value>) -> Result<(), String> {
    let Some(stored) = stored else {
        return Ok(());
    };
    let mut base = serde_json::to_value(SwarmConfig::default()).map_err(|e| e.to_string())?;
    merge_json(&mut base, stored);
    serde_json::from_value::<SwarmConfig>(base)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_swarm_block_the_reader_rejects_is_never_saved_over() {
        assert!(stored_block_deserializes(None).is_ok());
        assert!(stored_block_deserializes(Some(serde_json::json!({}))).is_ok());
        assert!(stored_block_deserializes(Some(
            serde_json::json!({"planner_model": "operator-model"})
        ))
        .is_ok());
        let reason = stored_block_deserializes(Some(
            serde_json::json!({"planner_model": ["not", "a string"]}),
        ))
        .unwrap_err();
        assert!(reason.contains("invalid type"), "{reason}");
    }
}
