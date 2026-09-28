use super::api_client::TlsConfig;
use anyhow::Result;
use futures::future::BoxFuture;
pub use goose_providers::conversation::token_usage::{
    DraftStats, ProviderStats, ProviderUsage, Usage,
};
use serde::{Deserialize, Serialize};

pub const DEFAULT_PROVIDER_TIMEOUT_SECS: u64 = 600;

use crate::config::ExtensionConfig;
use utoipa::ToSchema;

use std::path::PathBuf;

pub use goose_providers::base::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum ProviderType {
    Preferred,
    Builtin,
    Declarative,
    Custom,
}

/// The folder a provider built OUTSIDE a session works in (the model inventory, a key test, a
/// setup probe): the named sessionless folder. Q-266: this was the process cwd — goosed's, since
/// Q-257 the shared $HOME — and every provider a session builds takes the session's folder through
/// `from_env_with_working_dir` instead.
pub(crate) fn sessionless_working_dir() -> Result<PathBuf> {
    crate::config::paths::Paths::sessionless_dir()
}

pub trait ProviderDef: ProviderDescriptor + Send + Sync {
    type Provider: Provider + 'static;

    fn from_env(
        extensions: Vec<ExtensionConfig>,
        tls_config: Option<TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>>
    where
        Self: Sized;

    fn from_env_with_working_dir(
        extensions: Vec<ExtensionConfig>,
        _working_dir: PathBuf,
        tls_config: Option<TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>>
    where
        Self: Sized,
    {
        Self::from_env(extensions, tls_config)
    }
}
