use super::*;
use crate::config::{Config, ConfigError};
use goose_sdk_types::custom_requests::{
    CustomProviderUpsertDto, ProviderConfigFieldUpdate, ProviderConfigSaveRequest,
};
use leanzero_link::controller_management::{ManagedProvider, ProviderField};

pub(super) async fn list() -> Result<ManagedProviders, ControllerError> {
    let config = Config::global();
    let mut providers = Vec::new();
    for (metadata, kind) in crate::providers::providers().await {
        let configured = crate::providers::configured::check_provider_configured(&metadata, kind);
        let mut fields = Vec::new();
        for key in metadata.config_keys {
            fields.push(field(config, key)?);
        }
        providers.push(ManagedProvider {
            provider_id: metadata.name,
            display_name: metadata.display_name,
            configured,
            fields,
        });
    }
    providers.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(ManagedProviders { providers })
}
pub(super) fn field(
    config: &Config,
    key: crate::providers::base::ConfigKey,
) -> Result<ProviderField, ControllerError> {
    let value: Option<Value> = match config.get(&key.name, key.secret) {
        Ok(value) => Some(value),
        Err(ConfigError::NotFound(_)) => None,
        Err(_) => {
            return Err(ControllerError::unavailable(
                "provider configuration could not be read",
            ))
        }
    };
    let is_set = value
        .as_ref()
        .is_some_and(|v| !v.is_null() && v.as_str() != Some(""));
    Ok(ProviderField {
        key: key.name,
        required: key.required,
        secret: key.secret,
        is_set,
        value: if key.secret {
            None
        } else {
            value.map(|v| match v {
                Value::String(s) => s,
                other => other.to_string(),
            })
        },
    })
}

pub(super) async fn execute(action: ManagementAction) -> Result<Option<String>, ControllerError> {
    let refused = |_: agent_client_protocol::Error| {
        error(
            StatusCode::CONFLICT,
            "provider_configuration_failed",
            "provider configuration was refused; check its fields and credentials on the host",
        )
    };
    match action {
        ManagementAction::ProviderConfigure {
            provider_id,
            fields,
            test_model,
        } => {
            if fields.is_empty() {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "at least one provider field is required",
                ));
            }
            let request = ProviderConfigSaveRequest {
                provider_id,
                fields: fields
                    .into_iter()
                    .map(|(key, value)| ProviderConfigFieldUpdate { key, value })
                    .collect(),
                test_model,
            };
            crate::acp::server::save_provider_config_core(&request)
                .await
                .map_err(refused)?;
            Ok(None)
        }
        ManagementAction::ProviderCreate {
            engine,
            display_name,
            api_url,
            api_key,
            models,
            requires_auth,
        } => {
            let provider = CustomProviderUpsertDto {
                engine,
                display_name,
                api_url,
                api_key,
                models,
                requires_auth,
                supports_streaming: None,
                headers: Default::default(),
                catalog_provider_id: None,
                base_path: None,
                preserves_thinking: None,
            };
            crate::acp::server::create_provider_core(provider)
                .await
                .map(Some)
                .map_err(refused)
        }
        _ => Err(error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unsupported_action",
            "unsupported provider operation",
        )),
    }
}
