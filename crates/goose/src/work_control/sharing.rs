//! Read-only recipient observations. Unknown routing is never inferred from a provider label.

use super::OwnerRunRegistry;
use crate::agents::extension::ExtensionConfig;
use crate::agents::Agent;
use leanzero_link::controller_wire::{
    SharingProvenance, SharingProvider, SharingTool, WorkKind, WorkSharingDescriptor,
};

impl OwnerRunRegistry {
    pub(super) async fn sharing_descriptor(
        &self,
        session_id: &str,
        kind: WorkKind,
    ) -> anyhow::Result<WorkSharingDescriptor> {
        if kind != WorkKind::Chat {
            return Ok(unavailable("external_execution_not_reported"));
        }
        let mut agent = self.active_agent(session_id).await;
        if agent.is_none() {
            if let Some(manager) = self.agent_manager.get() {
                agent = manager.cached_agent(session_id).await;
            }
        }
        if let Some(agent) = agent {
            return Ok(live_descriptor(&agent).await);
        }
        let session = self.session_manager.get_session(session_id, false).await?;
        match session.provider_name.and_then(|name| safe_label(&name)) {
            Some(name) => Ok(WorkSharingDescriptor {
                provenance: SharingProvenance::Saved,
                provider: Some(SharingProvider { name, origin: None }),
                tools: Vec::new(),
                limitations: vec![
                    "saved_configuration_not_active".into(),
                    "provider_destination_unavailable".into(),
                    "tool_destinations_dynamic".into(),
                ],
            }),
            None => Ok(unavailable("session_runtime_unavailable")),
        }
    }
}

fn unavailable(reason: &str) -> WorkSharingDescriptor {
    WorkSharingDescriptor {
        provenance: SharingProvenance::Unavailable,
        provider: None,
        tools: Vec::new(),
        limitations: vec![
            reason.into(),
            "provider_destination_unavailable".into(),
            "tool_destinations_dynamic".into(),
        ],
    }
}

async fn live_descriptor(agent: &Agent) -> WorkSharingDescriptor {
    let provider = match agent.provider().await {
        Ok(provider) => safe_label(provider.get_name()).map(|name| SharingProvider {
            name,
            origin: provider
                .sharing_origin()
                .as_deref()
                .and_then(sanitized_origin),
        }),
        Err(_) => None,
    };
    let mut limitations = vec![
        "provider_routing_not_reported".into(),
        "tool_destinations_dynamic".into(),
    ];
    if provider
        .as_ref()
        .is_none_or(|provider| provider.origin.is_none())
    {
        limitations.push("provider_destination_unavailable".into());
    }
    let mut tools = agent
        .extension_manager
        .project_resolved_configs(tool_descriptor)
        .await;
    tools.extend(
        agent
            .get_extension_configs()
            .await
            .iter()
            .filter(|config| matches!(config, ExtensionConfig::Frontend { .. }))
            .map(|config| tool_descriptor(config, None)),
    );
    if tools
        .iter()
        .flatten()
        .any(|tool| tool.transport == "http" && tool.origin.is_none())
        || tools.iter().any(Option::is_none)
    {
        limitations.push("tool_endpoint_unavailable".into());
    }
    let mut tools: Vec<_> = tools.into_iter().flatten().collect();
    tools.sort_by(|a, b| {
        (&a.name, &a.transport, &a.origin).cmp(&(&b.name, &b.transport, &b.origin))
    });
    tools.dedup();
    WorkSharingDescriptor {
        provenance: SharingProvenance::Live,
        provider,
        tools,
        limitations,
    }
}

fn tool_descriptor(
    config: &ExtensionConfig,
    connected_http_uri: Option<&str>,
) -> Option<SharingTool> {
    let (transport, origin) = match config {
        ExtensionConfig::StreamableHttp { .. } | ExtensionConfig::Sse { .. } => {
            ("http", connected_http_uri.and_then(sanitized_origin))
        }
        ExtensionConfig::Stdio { .. } => ("stdio", None),
        ExtensionConfig::Builtin { .. } => ("builtin", None),
        ExtensionConfig::Platform { .. } => ("platform", None),
        ExtensionConfig::Frontend { .. } => ("frontend", None),
        ExtensionConfig::InlinePython { .. } => ("inline_python", None),
    };
    Some(SharingTool {
        name: safe_label(&config.name())?,
        transport: transport.into(),
        origin,
    })
}

fn safe_label(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | '@' | '?' | '#')))
    .then(|| value.to_owned())
}

fn sanitized_origin(value: &str) -> Option<String> {
    if value.contains("${") || value.chars().any(char::is_control) {
        return None;
    }
    let url = url::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{AgentConfig, GoosePlatform};
    use crate::providers::base::{MessageStream, Provider};
    use goose_providers::errors::ProviderError;
    use std::sync::Arc;

    #[test]
    fn recipient_sharing_origins_strip_credentials_and_reject_non_endpoints() {
        assert_eq!(
            sanitized_origin(
                "https://account:secret@proxy.example:8443/private/token?q=secret#secret"
            ),
            Some("https://proxy.example:8443".into())
        );
        assert_eq!(
            sanitized_origin("http://[::1]:11434/v1"),
            Some("http://[::1]:11434".into())
        );
        for invalid in [
            "/private/secret",
            "relative.example",
            "file:///private/secret",
            "javascript:secret",
            "https://",
            "https://${HOST}/v1",
        ] {
            assert_eq!(sanitized_origin(invalid), None, "{invalid}");
        }
        for invalid in [
            "",
            "/private/secret",
            "https://user:secret@example.test",
            "label\nsecret",
        ] {
            assert_eq!(safe_label(invalid), None);
        }
    }

    #[test]
    fn recipient_sharing_tool_transport_does_not_imply_local_or_expose_config() {
        let config: ExtensionConfig = serde_json::from_value(serde_json::json!({
            "type": "streamable_http", "name": "Issue service", "uri": "https://user:secret@tools.example/mcp/secret?key=secret",
            "headers": {"Authorization": "Bearer secret"}, "envs": {"TOKEN": "secret"}
        })).unwrap();
        let tool = tool_descriptor(
            &config,
            Some("https://user:secret@tools.example/mcp/secret?key=secret"),
        )
        .unwrap();
        assert_eq!(tool.origin.as_deref(), Some("https://tools.example"));
        assert!(!serde_json::to_string(&tool).unwrap().contains("secret"));
        let mut uds = config;
        if let ExtensionConfig::StreamableHttp { socket, .. } = &mut uds {
            *socket = Some("/private/secret.sock".into());
        }
        assert_eq!(tool_descriptor(&uds, None).unwrap().origin, None);
        let stdio: ExtensionConfig = serde_json::from_value(serde_json::json!({
            "type": "stdio", "name": "Shell tools", "cmd": "/private/secret", "args": ["secret"], "envs": {"TOKEN": "secret"}
        })).unwrap();
        let tool = tool_descriptor(&stdio, None).unwrap();
        assert_eq!(tool.transport, "stdio");
        assert_eq!(tool.origin, None);
        assert!(!serde_json::to_string(&tool).unwrap().contains("secret"));
    }

    #[tokio::test]
    async fn recipient_sharing_saved_and_unknown_reads_do_not_load_an_agent() {
        let fixture = super::super::tests::Fixture::new().await;
        let executor = fixture.runs.executor.read().unwrap().clone().unwrap();
        let (_, manager) = executor.prepare(&fixture.session).await.unwrap();
        fixture.runs.bind_agent_manager(manager.clone());
        let unknown = fixture
            .runs
            .sharing_descriptor(&fixture.session, WorkKind::Chat)
            .await
            .unwrap();
        assert_eq!(unknown.provenance, SharingProvenance::Unavailable);
        fixture
            .manager
            .update(&fixture.session)
            .provider_name("saved-driver")
            .apply()
            .await
            .unwrap();
        let saved = fixture
            .runs
            .sharing_descriptor(&fixture.session, WorkKind::Chat)
            .await
            .unwrap();
        assert_eq!(saved.provenance, SharingProvenance::Saved);
        assert_eq!(
            saved.provider.unwrap(),
            SharingProvider {
                name: "saved-driver".into(),
                origin: None
            }
        );
        assert!(saved
            .limitations
            .contains(&"saved_configuration_not_active".into()));
        assert!(manager.cached_agent(&fixture.session).await.is_none());
        assert_eq!(manager.session_count().await, 0);
        let external = fixture
            .runs
            .sharing_descriptor("not-a-chat-session", WorkKind::AgentWork)
            .await
            .unwrap();
        assert_eq!(external.provenance, SharingProvenance::Unavailable);
        assert!(external
            .limitations
            .contains(&"external_execution_not_reported".into()));
        let _ticket = fixture.reserve_run().await;
        let live = fixture
            .runs
            .sharing_descriptor(&fixture.session, WorkKind::Chat)
            .await
            .unwrap();
        assert_eq!(live.provenance, SharingProvenance::Live);
        assert_eq!(live.provider, None);
        assert!(live
            .limitations
            .contains(&"provider_destination_unavailable".into()));
    }

    struct ObservedProvider;

    #[async_trait::async_trait]
    impl Provider for ObservedProvider {
        async fn stream(
            &self,
            _: &goose_providers::model::ModelConfig,
            _: &str,
            _: &[crate::conversation::message::Message],
            _: &[rmcp::model::Tool],
        ) -> Result<MessageStream, ProviderError> {
            panic!("a sharing snapshot must not invoke inference");
        }

        fn get_name(&self) -> &str {
            "observed-driver"
        }
        fn endpoint(&self) -> Option<&str> {
            Some("https://configured-host.example")
        }
        fn sharing_origin(&self) -> Option<String> {
            Some("https://user:secret@actual-proxy.example/v1/secret?key=secret".into())
        }
    }

    #[tokio::test]
    async fn recipient_sharing_live_provider_uses_driver_observation_and_qualifies_routing() {
        let fixture = super::super::tests::Fixture::new().await;
        let agent = Agent::with_config(AgentConfig::new(
            fixture.manager.clone(),
            crate::config::permission::PermissionManager::instance(),
            None,
            crate::config::GooseMode::Auto,
            true,
            GoosePlatform::GooseCli,
        ));
        agent
            .update_provider(
                Arc::new(ObservedProvider),
                goose_providers::model::ModelConfig::new("fixture-model"),
                &fixture.session,
            )
            .await
            .unwrap();
        let descriptor = live_descriptor(&agent).await;
        assert_eq!(descriptor.provenance, SharingProvenance::Live);
        assert_eq!(
            descriptor.provider.as_ref().unwrap(),
            &SharingProvider {
                name: "observed-driver".into(),
                origin: Some("https://actual-proxy.example".into()),
            }
        );
        assert!(descriptor
            .limitations
            .contains(&"provider_routing_not_reported".into()));
        assert!(descriptor
            .limitations
            .contains(&"tool_destinations_dynamic".into()));
        assert!(!serde_json::to_string(&descriptor)
            .unwrap()
            .contains("secret"));
    }
}
