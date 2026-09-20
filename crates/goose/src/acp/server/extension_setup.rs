use super::*;
use crate::agents::extension_manager::ExtensionManager;
use rmcp::model::CallToolRequestParams;

impl GooseAcpAgent {
    pub(super) async fn on_inspect_config_extension(
        &self,
        req: InspectConfigExtensionRequest,
    ) -> Result<InspectConfigExtensionResponse, agent_client_protocol::Error> {
        let config =
            crate::config::extensions::get_extension_by_name(&req.name).ok_or_else(|| {
                agent_client_protocol::Error::invalid_params()
                    .data("Save the extension before testing its connection.")
            })?;
        let config = config.resolve(Config::global()).await.internal_err()?;
        let settings: std::collections::BTreeMap<String, String> = match &config {
            ExtensionConfig::Stdio { envs, .. } => envs
                .get_env()
                .into_iter()
                .filter(|(key, _)| {
                    matches!(
                        key.as_str(),
                        "OUTPUT_DIR"
                            | "DOC_OUTPUT_DIR"
                            | "CRAWL_CACHE_DIR"
                            | "Z_AI_BASE_URL"
                            | "Z_AI_VISION_MODEL"
                            | "WEB_SEARCH_MCP_URL"
                    )
                })
                .collect(),
            _ => std::collections::BTreeMap::new(),
        };
        if req.settings_only {
            return Ok(InspectConfigExtensionResponse {
                settings,
                tools: vec![],
                saved_file: None,
            });
        }
        let key = config.key();
        let manager = Arc::new(ExtensionManager::new_without_provider(Paths::data_dir()));
        manager.add_extension(config, None, None, None).await
            .map_err(|_| agent_client_protocol::Error::internal_error().data("The MCP server could not connect. Check its executable, credentials and endpoint."))?;
        let result = async {
            let tools = manager.get_prefixed_tools("mcp-setup", Some(key.clone())).await.internal_err()?;
            let mut saved_file = None;
            if let Some(source) = req.source_url {
                let url = url::Url::parse(&source).map_err(|_| agent_client_protocol::Error::invalid_params().data("Enter a valid source URL."))?;
                if !matches!(url.scheme(), "https" | "http") || !url.username().is_empty() || url.password().is_some() {
                    return Err(agent_client_protocol::Error::invalid_params().data("Use an HTTP or HTTPS URL without embedded credentials."));
                }
                let tool_name = format!("{key}__get-single-web-page-content");
                if !tools.iter().any(|tool| tool.name == tool_name) {
                    return Err(agent_client_protocol::Error::invalid_params().data("This server does not provide page extraction."));
                }
                let folder = settings.get("OUTPUT_DIR").ok_or_else(|| agent_client_protocol::Error::invalid_params().data("Save a research corpus folder before collecting a source."))?;
                let folder = PathBuf::from(folder);
                if !folder.is_absolute() {
                    return Err(agent_client_protocol::Error::invalid_params().data("The corpus folder must be an absolute path."));
                }
                // Use the bundled server's research directory so list-cached-documents
                // and read-cached-document can consume sources collected in the UI.
                let folder = folder.join("docs").join("research-output");
                let ctx = crate::agents::ToolCallContext::new("mcp-setup".into(), None, None);
                let call = CallToolRequestParams::new(tool_name).with_arguments(serde_json::json!({"url": url.as_str()}).as_object().expect("object literal").clone());
                let response = manager.dispatch_tool_call(&ctx, call, CancellationToken::new()).await.internal_err()?.result.await.internal_err()?;
                if response.is_error == Some(true) {
                    return Err(agent_client_protocol::Error::internal_error().data("The MCP server could not extract this page. No source was saved."));
                }
                let text = response.content.iter().filter_map(|content| content.as_text().map(|text| text.text.as_str())).collect::<Vec<_>>().join("\n\n");
                if text.trim().is_empty() {
                    return Err(agent_client_protocol::Error::internal_error().data("The MCP server returned an empty page. No source was saved."));
                }
                tokio::fs::create_dir_all(&folder).await.internal_err()?;
                let file = folder.join(format!("source-{}.md", uuid::Uuid::new_v4()));
                let body = format!("# Collected web source\n\nSource: {url}\nCollected: {}\nServer: {}\n\n{text}\n", chrono::Utc::now().to_rfc3339(), req.name);
                use tokio::io::AsyncWriteExt;
                let mut output = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&file).await.internal_err()?;
                output.write_all(body.as_bytes()).await.internal_err()?;
                saved_file = Some(file.to_string_lossy().into_owned());
            }
            let tools = tools.into_iter().map(|tool| serde_json::json!({
                "name": tool.name.strip_prefix(&format!("{key}__")).unwrap_or(&tool.name),
                "description": tool.description,
                "inputSchema": tool.input_schema,
            })).collect();
            Ok(InspectConfigExtensionResponse { settings, tools, saved_file })
        }.await;
        manager.remove_extension(&key).await.internal_err()?;
        result
    }
}
