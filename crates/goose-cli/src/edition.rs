//! Goose Local Edition resolution.
//!
//! An `Edition` (`Standard` | `Local`) selects the "goose" vs "Goose Local Edition" UX skin. It is a
//! presentation choice only — it never gates model capability. Resolution precedence, highest first:
//!
//! 1. the `--local` CLI flag,
//! 2. the `GOOSE_LOCAL_EDITION` environment variable,
//! 3. the persisted `config.yaml` `edition` key,
//! 4. a DERIVED default — a local/swarm provider (LM Studio, Ollama, a swarm) implies Local, so the
//!    "Local" name is *true* rather than a badge divorced from locality. There is no interactive
//!    first-run prompt: the calm home is never interrupted for a skin choice.

use goose::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edition {
    Standard,
    Local,
}

impl Edition {
    pub fn is_local(self) -> bool {
        matches!(self, Edition::Local)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Edition::Standard => "standard",
            Edition::Local => "local",
        }
    }
}

/// Provider-name fragments that are inherently local — their presence derives Local Edition.
const LOCAL_PROVIDER_FRAGMENTS: &[&str] = &["lmstudio", "ollama", "swarm", "llama", "localai"];

/// Interpret an environment-variable string as a boolean flag. Absent -> None; a falsey token
/// (empty / `0` / `false` / `no` / `off`) -> Some(false); anything else -> Some(true).
fn env_bool(name: &str) -> Option<bool> {
    std::env::var(name).ok().map(|v| {
        let v = v.trim().to_ascii_lowercase();
        !(v.is_empty() || v == "0" || v == "false" || v == "no" || v == "off")
    })
}

/// True if a provider name reads as a local/swarm backend.
fn provider_is_local(provider: &str) -> bool {
    let p = provider.to_ascii_lowercase();
    LOCAL_PROVIDER_FRAGMENTS.iter().any(|frag| p.contains(frag))
}

/// PURE resolver — all inputs supplied, no global state — so the precedence is unit-testable.
///
/// `cli_local` is a plain bool: a `--local` bool flag can only signal "make it Local" (true), so a
/// false value means "not requested" and falls through to the next source, exactly per the precedence.
pub fn resolve_edition_from(
    cli_local: bool,
    env_local: Option<bool>,
    persisted: Option<&str>,
    active_provider: Option<&str>,
) -> Edition {
    if cli_local {
        return Edition::Local;
    }
    if let Some(env) = env_local {
        return if env {
            Edition::Local
        } else {
            Edition::Standard
        };
    }
    match persisted.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("local") => return Edition::Local,
        Some("standard") => return Edition::Standard,
        _ => {}
    }
    if active_provider.map(provider_is_local).unwrap_or(false) {
        return Edition::Local;
    }
    Edition::Standard
}

/// Resolve the active edition, reading the env / persisted config / active provider best-effort.
pub fn resolve_edition(cli_local: bool) -> Edition {
    let config = Config::global();
    let persisted: Option<String> = config.get_param("edition").ok();
    let provider: Option<String> = config.get_goose_provider().ok();
    resolve_edition_from(
        cli_local,
        env_bool("GOOSE_LOCAL_EDITION"),
        persisted.as_deref(),
        provider.as_deref(),
    )
}

/// Persist the chosen edition to `config.yaml` so a later invocation resolves to it without a flag.
pub fn set_edition(edition: Edition) -> Result<(), goose::config::ConfigError> {
    Config::global().set_param("edition", edition.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_flag_wins_over_everything() {
        // --local forces Local even when env/config/provider all say standard.
        assert_eq!(
            resolve_edition_from(true, Some(false), Some("standard"), Some("openai")),
            Edition::Local
        );
    }

    #[test]
    fn env_overrides_config_and_derivation() {
        // env=false beats a persisted `local` and a local provider.
        assert_eq!(
            resolve_edition_from(false, Some(false), Some("local"), Some("lmstudio")),
            Edition::Standard
        );
        assert_eq!(
            resolve_edition_from(false, Some(true), Some("standard"), Some("openai")),
            Edition::Local
        );
    }

    #[test]
    fn persisted_config_beats_derivation() {
        assert_eq!(
            resolve_edition_from(false, None, Some("standard"), Some("lmstudio")),
            Edition::Standard
        );
        assert_eq!(
            resolve_edition_from(false, None, Some("local"), Some("openai")),
            Edition::Local
        );
    }

    #[test]
    fn derives_local_from_a_local_provider() {
        for p in [
            "lmstudio",
            "LMStudio",
            "ollama",
            "swarm",
            "my-llama-host",
            "localai",
        ] {
            assert_eq!(
                resolve_edition_from(false, None, None, Some(p)),
                Edition::Local,
                "provider {p} should derive Local"
            );
        }
    }

    #[test]
    fn defaults_to_standard_for_a_cloud_provider_or_none() {
        assert_eq!(
            resolve_edition_from(false, None, None, Some("anthropic")),
            Edition::Standard
        );
        assert_eq!(
            resolve_edition_from(false, None, None, None),
            Edition::Standard
        );
    }

    #[test]
    fn unrecognized_persisted_value_falls_through_to_derivation() {
        assert_eq!(
            resolve_edition_from(false, None, Some("weird"), Some("ollama")),
            Edition::Local
        );
    }
}
