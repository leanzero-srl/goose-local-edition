//! Q-302: a provider error ends the turn as a notice with its class on the message's metadata, and
//! the closing sentence advises a retry only for a class a resend can outlive.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use goose::agents::split_record::{PERMANENT_ERROR_CLOSER, TRANSIENT_ERROR_CLOSER};
use goose::agents::{Agent, AgentConfig, GoosePlatform, SessionConfig};
use goose::config::permission::PermissionManager;
use goose::config::GooseMode;
use goose::conversation::message::{Message, ProviderErrorNotice};
use goose::providers::base::{
    MessageStream, Provider, ProviderDef, ProviderDescriptor, ProviderMetadata,
};
use goose::session::{SessionManager, SessionType};
use goose_providers::errors::{http_failure_text, ProviderError};
use goose_providers::model::ModelConfig;
use rmcp::model::Tool;

#[ctor::ctor]
fn hermetic_path_root() {
    goose_test_support::hermetic_path_root();
}

/// Refuses every call the way Rapid-MLX refused an image part on 3.0.68 (70-failed-session.png).
struct RefusingProvider;

impl ProviderDescriptor for RefusingProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata {
            name: "refusing-mock".to_string(),
            display_name: "Refusing Mock".to_string(),
            description: "Refuses".to_string(),
            default_model: "mock-model".to_string(),
            known_models: vec![],
            model_doc_link: String::new(),
            config_keys: vec![],
            setup_steps: vec![],
            model_selection_hint: None,
            fast_model: None,
        }
    }
}

impl ProviderDef for RefusingProvider {
    type Provider = Self;

    fn from_env(
        _extensions: Vec<goose::config::ExtensionConfig>,
        _tls_config: Option<goose::providers::api_client::TlsConfig>,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<Self>> {
        unimplemented!()
    }
}

const URL: &str = "http://127.0.0.1:8091/v1/chat/completions";
const SAID: &str = "Only 'text' content type is supported.";

#[async_trait]
impl Provider for RefusingProvider {
    async fn stream(
        &self,
        _model_config: &ModelConfig,
        _system_prompt: &str,
        _messages: &[Message],
        _tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        Err(ProviderError::RequestFailed(http_failure_text(
            "Resource not found (404)",
            URL,
            SAID,
        )))
    }

    fn get_name(&self) -> &str {
        "refusing-mock"
    }
}

#[tokio::test]
async fn a_permanent_refusal_ends_the_turn_as_a_classed_notice_with_no_retry_advice() -> Result<()>
{
    let temp_dir = tempfile::tempdir()?;
    let session_manager = Arc::new(SessionManager::new(temp_dir.path().to_path_buf()));
    let agent = Agent::with_config(AgentConfig::new(
        session_manager.clone(),
        PermissionManager::instance(),
        None,
        GooseMode::Auto,
        true,
        GoosePlatform::GooseDesktop,
    ));
    let session = session_manager
        .create_session(
            PathBuf::default(),
            "refused".to_string(),
            SessionType::User,
            GooseMode::default(),
        )
        .await?;
    agent
        .update_provider(
            Arc::new(RefusingProvider),
            ModelConfig::new("mock-model"),
            &session.id,
        )
        .await?;

    let reply = agent
        .reply(
            Message::user().with_text("crop the image"),
            SessionConfig {
                id: session.id.clone(),
                schedule_id: None,
                max_turns: None,
                retry_config: None,
            },
            None,
        )
        .await?;
    tokio::pin!(reply);
    while let Some(event) = reply.next().await {
        event?;
    }

    let conversation = session_manager
        .get_session(&session.id, true)
        .await?
        .conversation
        .expect("the session has its conversation");
    let last = conversation
        .messages()
        .last()
        .expect("the failure is saved");
    assert_eq!(
        last.metadata.provider_error,
        Some(Box::new(ProviderErrorNotice {
            class: "request".to_string(),
            transient: false,
            said: SAID.to_string(),
            detail: format!("Request failed: Resource not found (404) at {URL}: {SAID}"),
        }))
    );
    let text = last.as_concat_text();
    assert!(text.ends_with(PERMANENT_ERROR_CLOSER), "{text}");
    assert!(!text.contains(TRANSIENT_ERROR_CLOSER), "{text}");
    assert!(
        !last.is_agent_visible(),
        "a notice is never sent to the model"
    );
    Ok(())
}
