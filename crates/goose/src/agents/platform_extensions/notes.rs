//! `send_note`: the person asks this chat to tell another chat something (Q-358 part 2). The tool
//! SENDS NOTHING — it resolves the other chat and pins a draft here; the person's click on the draft
//! is what sends it (`chat_notes::send`, reached only from the desktop's `notes/send`).

use crate::agents::extension::PlatformExtensionContext;
use crate::agents::mcp_client::{Error, McpClientTrait};
use crate::agents::tool_execution::ToolCallContext;
use crate::chat_notes::{self, home_relative, DraftNote, TargetResolution};
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

pub static EXTENSION_NAME: &str = "notes";
pub const SEND_NOTE_TOOL_NAME: &str = "send_note";

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SendNoteParams {
    /// The other chat, in the person's words: its title or what it is about ("the billing
    /// migration chat").
    pub to: String,
    /// What the other chat should know, written so it stands on its own there: the finding, the
    /// file or name it concerns, and why it matters to that work.
    pub text: String,
}

pub struct NotesClient {
    info: InitializeResult,
    context: PlatformExtensionContext,
}

impl NotesClient {
    pub fn new(context: PlatformExtensionContext) -> Result<Self> {
        let info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(EXTENSION_NAME.to_string(), "1.0.0".to_string())
                    .with_title("Notes to other chats"),
            )
            .with_instructions(
                indoc! {r#"
                When the person asks you to tell, pass on to, or leave a note for ANOTHER of their
                chats ("tell the migration chat that tenant_id is the new column"), call
                `send_note`. Only when they ask: never send a note on your own initiative.
                `send_note` sends nothing. It pins a draft in this chat, addressed to the chat it
                found, and the person decides there: steer it into that chat now, leave it there,
                pick another chat, or cancel. You are told what became of it.
            "#}
                .to_string(),
            );
        Ok(Self { info, context })
    }

    fn tools() -> Vec<Tool> {
        let schema = schema_for!(SendNoteParams);
        let schema_value =
            serde_json::to_value(schema).expect("Failed to serialize SendNoteParams schema");
        vec![Tool::new(
            SEND_NOTE_TOOL_NAME.to_string(),
            indoc! {r#"
                Draft a note to another of the person's chats, when the person asks you to tell
                that chat something. Nothing is sent: the draft is pinned in this chat and only the
                person's click sends it. Your turn goes on.
            "#}
            .to_string(),
            schema_value.as_object().unwrap().clone(),
        )
        .annotate(ToolAnnotations::from_raw(
            Some("Draft a note to another chat".to_string()),
            Some(false),
            Some(false),
            Some(true),
            Some(false),
        ))]
    }

    async fn send_note(
        &self,
        session_id: &str,
        arguments: Option<JsonObject>,
    ) -> Result<CallToolResult, String> {
        let params: SendNoteParams = serde_json::from_value(serde_json::Value::Object(
            arguments.ok_or("Missing arguments")?,
        ))
        .map_err(|e| format!("Invalid arguments: {e}"))?;
        let draft = chat_notes::draft(
            &self.context.session_manager,
            session_id,
            &params.to,
            &params.text,
        )
        .await
        .map_err(|e| e.to_string())?;
        let result = CallToolResult::success(vec![Content::text(result_text(&draft))]);
        let mut event = serde_json::Map::new();
        event.insert("session_id".to_string(), session_id.into());
        event.insert("note_id".to_string(), draft.id.clone().into());
        chat_notes::hub().changed(&[session_id.to_string()]);
        Ok(self.context.result_with_platform_notification(
            result,
            EXTENSION_NAME,
            "note_drafted",
            event,
        ))
    }
}

/// What the model reads back: that nothing was sent, where the draft points, and what happens next
/// — or, when no chat could be named, that plainly.
fn result_text(draft: &DraftNote) -> String {
    let nothing_sent = "Nothing has been sent.";
    match (&draft.target, draft.resolution) {
        (Some(target), _) => format!(
            "{nothing_sent} A draft note to the chat \"{}\" ({}) is pinned in this chat as {}. \
             The person decides there: steer it into that chat now, leave it there, pick another \
             chat, or cancel. You will be told what became of it; do not repeat the note in text.",
            target.name,
            home_relative(&target.working_dir),
            draft.id
        ),
        (None, TargetResolution::Ambiguous) => format!(
            "{nothing_sent} Several chats match \"{}\" equally: {}. The draft ({}) is pinned in \
             this chat with them listed for the person to pick one.",
            draft.to_query,
            draft
                .candidates
                .iter()
                .map(|chat| format!("\"{}\" ({})", chat.name, home_relative(&chat.working_dir)))
                .collect::<Vec<_>>()
                .join(", "),
            draft.id
        ),
        (None, _) => format!(
            "{nothing_sent} No chat's title or folder matches \"{}\". The draft ({}) is pinned \
             in this chat without a chat to go to; the person can pick one there, or ask them \
             which chat they meant.",
            draft.to_query, draft.id
        ),
    }
}

#[async_trait]
impl McpClientTrait for NotesClient {
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
            SEND_NOTE_TOOL_NAME => self.send_note(&ctx.session_id, arguments).await,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_notes::{ChatNotesState, DraftStatus};
    use crate::session::extension_data::ExtensionState;
    use crate::session::{SessionManager, SessionType};
    use std::sync::Arc;

    async fn chats(manager: &SessionManager, dir: &std::path::Path, names: &[&str]) -> Vec<String> {
        let mut ids = Vec::new();
        for name in names {
            let session = manager
                .create_session(
                    dir.to_path_buf(),
                    name.to_string(),
                    SessionType::User,
                    crate::config::GooseMode::default(),
                )
                .await
                .unwrap();
            ids.push(session.id);
        }
        ids
    }

    fn client(manager: Arc<SessionManager>) -> NotesClient {
        NotesClient::new(PlatformExtensionContext {
            extension_manager: None,
            session_manager: manager,
            session: None,
            use_login_shell_path: false,
            working_dir: None,
        })
        .unwrap()
    }

    /// HUMAN DIRECTION IS STRUCTURAL. The one tool a model has drafts; it cannot send. After the
    /// model calls it, the target chat holds nothing — no inbox note, no message — and the draft
    /// waits in the sender for the person's click. The only writer of another chat's inbox is
    /// `chat_notes::send`, which no tool reaches.
    #[tokio::test]
    async fn a_model_can_only_draft_a_note_never_deliver_one() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(SessionManager::new(dir.path().to_path_buf()));
        let ids = chats(
            &manager,
            dir.path(),
            &["Explore split mesh", "Migrate billing"],
        )
        .await;
        let (sender, target) = (&ids[0], &ids[1]);
        let notes = client(manager.clone());

        let names: Vec<String> = NotesClient::tools()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect();
        assert_eq!(
            names,
            vec![SEND_NOTE_TOOL_NAME],
            "the model's only tool drafts"
        );

        let mut args = JsonObject::new();
        args.insert("to".into(), "the billing chat".into());
        args.insert("text".into(), "tenant_id is the new column".into());
        let result = notes
            .call_tool(
                &ToolCallContext::new(sender.clone(), None, None),
                SEND_NOTE_TOOL_NAME,
                Some(args),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_ne!(result.is_error, Some(true), "{result:?}");
        let said = result.content[0].as_text().unwrap().text.clone();
        assert!(said.starts_with("Nothing has been sent."), "{said}");
        assert!(said.contains("\"Migrate billing\""), "{said}");

        let target_session = manager.get_session(target, true).await.unwrap();
        let target_notes =
            ChatNotesState::from_extension_data(&target_session.extension_data).unwrap_or_default();
        assert!(
            target_notes.inbox.is_empty(),
            "the tool wrote nothing to the other chat"
        );
        assert!(
            target_session
                .conversation
                .map(|c| c.messages().is_empty())
                .unwrap_or(true),
            "the other chat's conversation is untouched"
        );

        let sender_session = manager.get_session(sender, false).await.unwrap();
        let drafts = ChatNotesState::from_extension_data(&sender_session.extension_data)
            .unwrap()
            .drafts;
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].status, DraftStatus::Draft);
        assert_eq!(
            drafts[0].target.as_ref().map(|t| t.session_id.as_str()),
            Some(target.as_str())
        );
    }

    #[tokio::test]
    async fn no_matching_chat_is_said_plainly() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(SessionManager::new(dir.path().to_path_buf()));
        let ids = chats(
            &manager,
            dir.path(),
            &["Explore split mesh", "Migrate billing"],
        )
        .await;
        let notes = client(manager.clone());
        let mut args = JsonObject::new();
        args.insert("to".into(), "payments".into());
        args.insert("text".into(), "x".into());
        let result = notes
            .call_tool(
                &ToolCallContext::new(ids[0].clone(), None, None),
                SEND_NOTE_TOOL_NAME,
                Some(args),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let said = result.content[0].as_text().unwrap().text.clone();
        assert!(
            said.contains("No chat's title or folder matches \"payments\""),
            "{said}"
        );
    }
}
