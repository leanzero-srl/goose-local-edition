pub use goose_providers::local_inference::*;

use crate::config::ExtensionConfig;
use crate::providers::api_client::TlsConfig;
use crate::providers::base::{sessionless_working_dir, ProviderDef};
use anyhow::Result;
use futures::future::BoxFuture;
use std::path::PathBuf;

fn resolve_huggingface_token() -> BoxFuture<'static, Result<Option<String>>> {
    Box::pin(crate::providers::huggingface_auth::resolve_token_async())
}

fn resolve_string_param(key: &'static str) -> Result<Option<String>> {
    Ok(crate::config::Config::global()
        .get_param::<String>(key)
        .ok())
}

fn resolve_bool_param(key: &'static str) -> Result<Option<bool>> {
    Ok(crate::config::Config::global().get_param::<bool>(key).ok())
}

pub fn configure_local_inference() {
    huggingface_auth::set_token_resolver(resolve_huggingface_token);
    config_resolver::set_string_param_resolver(resolve_string_param);
    config_resolver::set_bool_param_resolver(resolve_bool_param);
}

pub fn configure_huggingface_auth() {
    configure_local_inference();
}

impl ProviderDef for LocalInferenceProvider {
    type Provider = Self;

    /// A provider built outside a session (the model inventory, a key test) works in the named
    /// sessionless folder, as every provider does since Q-266 — never goosed's cwd.
    fn from_env(
        extensions: Vec<ExtensionConfig>,
        tls_config: Option<TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>>
    where
        Self: Sized,
    {
        Box::pin(async move {
            <Self as ProviderDef>::from_env_with_working_dir(
                extensions,
                sessionless_working_dir()?,
                tls_config,
            )
            .await
        })
    }

    /// Q-284: the session's folder reaches the emulated-tools system prompt a small model is given.
    fn from_env_with_working_dir(
        _extensions: Vec<ExtensionConfig>,
        working_dir: PathBuf,
        _tls_config: Option<TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>>
    where
        Self: Sized,
    {
        configure_local_inference();
        Box::pin(Self::from_env(working_dir))
    }
}
