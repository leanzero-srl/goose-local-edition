use anyhow::{bail, Context, Result};
use goose::config::{declarative_providers, Config, ConfigError};
use goose::model_fields::MODEL_FIELDS_CONFIG_KEY;
use goose::providers::base::ProviderType;
use goose::providers::get_from_registry;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

fn optional(config: &Config, key: &str, secret: bool) -> Result<Option<Value>> {
    match config.get(key, secret) {
        Ok(value) => Ok(Some(value)),
        Err(ConfigError::NotFound(_)) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Transfer only entrant configuration; profiles, memories and extensions stay outside the sandbox.
pub async fn export(provider: Option<&str>, output: &Path) -> Result<()> {
    let config = Config::global();
    let mut params = Map::new();
    let mut secrets = Map::new();
    let mut definitions = Map::new();
    let mut providers = BTreeSet::new();
    if let Some(provider) = provider {
        providers.insert(provider.to_owned());
    } else {
        let swarm = optional(config, "swarm", false)?
            .context("Configure the swarm pool before starting a benchmark")?;
        if let Some(devices) = swarm.get("devices").and_then(Value::as_array) {
            for device in devices {
                if device.get("enabled").and_then(Value::as_bool) == Some(false) {
                    continue;
                }
                validate_isolated_device(device)?;
                providers.insert(
                    super::swarm::cloud::cloud_registry_name(
                        device
                            .get("provider")
                            .and_then(Value::as_str)
                            .unwrap_or("lmstudio"),
                    )
                    .to_owned(),
                );
            }
        }
        // Local pool discovery and the planner use the configured LM Studio endpoint.
        providers.insert("lmstudio".to_owned());
        params.insert("swarm".into(), swarm);
    }
    for name in &providers {
        let entry = get_from_registry(name).await?;
        for key in &entry.metadata().config_keys {
            match optional(config, &key.name, key.secret)? {
                Some(value) => {
                    if key.secret {
                        secrets.insert(key.name.clone(), value);
                    } else {
                        params.insert(key.name.clone(), value);
                    }
                }
                None if key.required && key.default.is_none() => {
                    bail!("Configure {name} in Settings: missing {}", key.name);
                }
                None => {}
            }
        }
        if name == "aws_bedrock" && !secrets.contains_key("AWS_BEARER_TOKEN_BEDROCK") {
            for key in [
                "AWS_ACCESS_KEY_ID",
                "AWS_SECRET_ACCESS_KEY",
                "AWS_SESSION_TOKEN",
            ] {
                if let Some(value) = optional(config, key, true)? {
                    secrets.insert(key.into(), value);
                }
            }
            if !secrets.contains_key("AWS_ACCESS_KEY_ID")
                || !secrets.contains_key("AWS_SECRET_ACCESS_KEY")
            {
                bail!("Benchmarks require a Bedrock API key or explicit AWS credentials; profile/SSO files are not copied into the isolated entrant");
            }
        }
        if matches!(entry.provider_type(), ProviderType::Custom) {
            let definition = declarative_providers::load_provider(name)?;
            definitions.insert(name.clone(), serde_json::to_value(definition.config)?);
        }
    }
    for key in [
        "GOOSE_TEMPERATURE",
        "GOOSE_CONTEXT_LIMIT",
        "GOOSE_MAX_TOKENS",
        "GOOSE_THINKING_EFFORT",
        "GOOSE_FAST_MODEL",
    ] {
        if let Some(value) = optional(config, key, false)? {
            params.insert(key.into(), value);
        }
    }
    if let Some(fields) = launcher_model_fields(
        std::env::var(MODEL_FIELDS_CONFIG_KEY).ok().as_deref(),
        &providers,
    )? {
        params.insert(MODEL_FIELDS_CONFIG_KEY.into(), fields);
    }
    let snapshot = json!({"version": 1, "providers": providers, "config": params,
                          "secrets": secrets, "custom_providers": definitions});
    write_private(output, &snapshot)
}

/// The per-model custom fields the benchmark LAUNCHER chose for this run (the desktop passes the
/// form's values in the environment), narrowed to the run's providers. The saved fields in
/// config.yaml are deliberately not read: a run launched without the variable (a hand-run
/// `run_build.py`, a tier whose effort is pinned for every entrant) carries none, so a saved
/// per-model effort can never displace the pin unnoticed. The snapshot's config becomes the
/// entrant's config.yaml, where the map is read like any saved value.
fn launcher_model_fields(raw: Option<&str>, providers: &BTreeSet<String>) -> Result<Option<Value>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let Value::Object(all) = serde_json::from_str::<Value>(raw)
        .with_context(|| format!("{MODEL_FIELDS_CONFIG_KEY} is not JSON"))?
    else {
        bail!("{MODEL_FIELDS_CONFIG_KEY} must be a JSON object of provider → model → fields");
    };
    let kept: Map<String, Value> = all
        .into_iter()
        .filter(|(provider, _)| providers.contains(provider))
        .collect();
    Ok((!kept.is_empty()).then_some(Value::Object(kept)))
}

fn validate_isolated_device(device: &Value) -> Result<()> {
    if device.get("engine").and_then(Value::as_str) == Some("mlx-sidecar") {
        bail!("Benchmark isolation cannot attach the managed MLX swarm engine; use Single model with its configured running API endpoint");
    }
    Ok(())
}

fn write_private(output: &Path, snapshot: &Value) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(output)?;
    file.write_all(serde_json::to_string(snapshot)?.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launchers_model_fields_ride_narrowed_to_the_runs_provider() {
        let providers = BTreeSet::from(["openrouter".to_string()]);
        assert_eq!(launcher_model_fields(None, &providers).unwrap(), None);

        let raw = r#"{"openrouter": {"deepseek/deepseek-v4.1-flash": {"effort": "low"}},
                      "anthropic": {"claude-x": {"effort": "high"}}}"#;
        assert_eq!(
            launcher_model_fields(Some(raw), &providers).unwrap(),
            Some(json!({"openrouter": {"deepseek/deepseek-v4.1-flash": {"effort": "low"}}}))
        );
        assert_eq!(
            launcher_model_fields(Some(r#"{"anthropic": {}}"#), &providers).unwrap(),
            None
        );
        assert!(launcher_model_fields(Some("[1]"), &providers).is_err());
        assert!(launcher_model_fields(Some("not json"), &providers).is_err());
    }

    #[test]
    fn managed_mlx_requires_attachment_but_api_devices_remain_available() {
        assert!(validate_isolated_device(&json!({"engine": "mlx-sidecar"})).is_err());
        assert!(validate_isolated_device(&json!({"provider": "omlx"})).is_ok());
        assert!(validate_isolated_device(&json!({"engine": "lmstudio"})).is_ok());
    }

    #[test]
    fn credential_snapshot_is_private_and_never_overwrites_existing_data() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("snapshot.json");
        write_private(&output, &json!({"secrets": {"KEY": "private"}})).unwrap();
        assert!(write_private(&output, &json!({})).is_err());
        let value: Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert_eq!(value["secrets"]["KEY"], "private");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(output).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
