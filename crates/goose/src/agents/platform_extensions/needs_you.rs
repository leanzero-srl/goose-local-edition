use crate::agents::extension::PlatformExtensionContext;
use crate::agents::mcp_client::{Error, McpClientTrait};
use crate::agents::tool_execution::ToolCallContext;
use crate::needs_you::{self, NewQuestion, END_TURN_META_KEY};
use anyhow::Result;
use async_trait::async_trait;
use indoc::indoc;
use rmcp::model::{
    CallToolResult, Content, Implementation, InitializeResult, JsonObject, ListToolsResult,
    ServerCapabilities, Tool, ToolAnnotations,
};
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

pub static EXTENSION_NAME: &str = "needs_you";
pub const ASK_USER_TOOL_NAME: &str = "ask_user";

/// The fields the card shows. Descriptions are what the model reads in the tool schema.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AskUserParams {
    /// The question, worded for the person: one decision or one piece of information.
    pub question: String,
    /// Why you need it: what it unblocks, and what changes with the answer.
    pub why: String,
    /// The answer you would pick yourself. Always fill it in: the person can accept it in one click.
    pub recommended_answer: String,
    /// Short alternatives when the answer is a choice between a few things. Omit for open questions.
    #[serde(default)]
    pub options: Vec<String>,
}

pub struct NeedsYouClient {
    info: InitializeResult,
    context: PlatformExtensionContext,
}

impl NeedsYouClient {
    pub fn new(context: PlatformExtensionContext) -> Result<Self> {
        let info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(EXTENSION_NAME.to_string(), "1.0.0".to_string())
                    .with_title("Needs you"),
            )
            .with_instructions(
                indoc! {r#"
                When you genuinely need something only the person can give — a decision between
                options with real trade-offs, a value or credential you cannot find, a requirement
                that is ambiguous in a way that changes the work, approval before something
                destructive or irreversible — call `ask_user`. Do not ask such questions in plain
                text: a question in prose scrolls away, while `ask_user` pins it where the person
                sees it until they answer.
                Always fill `recommended_answer` with what you would choose, and `why` with what the
                answer unblocks. Add `options` when the answer is one of a few choices.
                Do not ask for anything you can find out yourself by reading files, running commands
                or searching. Ask one question per call, and only when you cannot sensibly proceed.
                Calling `ask_user` ends your turn. The person's answer arrives as their next message.
            "#}
                .to_string(),
            );
        Ok(Self { info, context })
    }

    fn tools() -> Vec<Tool> {
        let schema = schema_for!(AskUserParams);
        let schema_value =
            serde_json::to_value(schema).expect("Failed to serialize AskUserParams schema");

        vec![Tool::new(
            ASK_USER_TOOL_NAME.to_string(),
            indoc! {r#"
                Ask the person a question you cannot answer yourself. It is pinned in their app
                until they answer or dismiss it, with your recommended answer offered as a
                one-click choice. Your turn ends when you call this; the answer arrives as the
                person's next message.
            "#}
            .to_string(),
            schema_value.as_object().unwrap().clone(),
        )
        .annotate(ToolAnnotations::from_raw(
            Some("Ask the user".to_string()),
            Some(true),
            Some(false),
            Some(false),
            Some(false),
        ))]
    }

    async fn ask_user(
        &self,
        session_id: &str,
        arguments: Option<JsonObject>,
    ) -> Result<CallToolResult, String> {
        let params: AskUserParams = serde_json::from_value(serde_json::Value::Object(
            arguments.ok_or("Missing arguments")?,
        ))
        .map_err(|e| format!("Invalid arguments: {e}"))?;

        let item = needs_you::raise(
            &self.context.session_manager,
            session_id,
            NewQuestion {
                question: params.question,
                why: params.why,
                recommended_answer: params.recommended_answer,
                options: params.options,
            },
        )
        .await
        .map_err(|e| e.to_string())?;

        let result = CallToolResult::success(vec![Content::text(format!(
            "The question is pinned in the person's app as item {}. Your turn ends now; do not \
             repeat the question in text. Their answer will arrive as their next message.",
            item.id
        ))]);
        let mut params = serde_json::Map::new();
        params.insert("session_id".to_string(), session_id.into());
        params.insert("item_id".to_string(), item.id.into());
        let mut result = self.context.result_with_platform_notification(
            result,
            EXTENSION_NAME,
            "needs_you_raised",
            params,
        );
        if let Some(meta) = result.meta.as_mut() {
            meta.0
                .insert(END_TURN_META_KEY.to_string(), serde_json::Value::Bool(true));
        }
        Ok(result)
    }
}

#[async_trait]
impl McpClientTrait for NeedsYouClient {
    async fn list_tools(
        &self,
        _session_id: &str,
        _next_cursor: Option<String>,
        _cancellation_token: CancellationToken,
    ) -> Result<ListToolsResult, Error> {
        Ok(ListToolsResult {
            tools: Self::tools(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        ctx: &ToolCallContext,
        name: &str,
        arguments: Option<JsonObject>,
        _cancellation_token: CancellationToken,
    ) -> Result<CallToolResult, Error> {
        let outcome = match name {
            ASK_USER_TOOL_NAME => self.ask_user(&ctx.session_id, arguments).await,
            _ => Err(format!("Unknown tool: {name}")),
        };
        Ok(outcome.unwrap_or_else(|error| {
            CallToolResult::error(vec![Content::text(format!("Error: {error}"))])
        }))
    }

    fn get_info(&self) -> Option<&InitializeResult> {
        Some(&self.info)
    }
}
