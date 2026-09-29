use super::discover_skills;
use super::loaded_skill_context_with_args;
use crate::agents::extension::PlatformExtensionContext;
use crate::agents::mcp_client::{Error, McpClientTrait};
use crate::agents::ToolCallContext;
use crate::conversation::message::{Message, MessageContent};
use crate::session::SessionManager;
use async_trait::async_trait;
use goose_sdk_types::custom_requests::{SourceEntry, SourceType};
use rmcp::model::{
    CallToolResult, Content, Implementation, InitializeResult, JsonObject, ListToolsResult,
    ServerCapabilities, ServerNotification, Tool,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub static EXTENSION_NAME: &str = "skills";

pub struct SkillsClient {
    info: InitializeResult,
    working_dir: PathBuf,
    exclude_builtin_skills: bool,
    session_manager: Arc<SessionManager>,
}

impl SkillsClient {
    /// Q-267: the folder was the session's or else the process cwd — goosed's, since Q-257 the
    /// shared $HOME. It is the folder the extension manager started this client for.
    pub fn new(context: PlatformExtensionContext) -> anyhow::Result<Self> {
        let working_dir = context
            .working_dir
            .clone()
            .or_else(|| context.session.as_ref().map(|s| s.working_dir.clone()))
            .ok_or_else(|| {
                anyhow::anyhow!("the skills extension was started without a folder, so it has no project skills to read")
            })?;

        let info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(EXTENSION_NAME, "1.0.0").with_title("Skills"));

        Ok(Self {
            info,
            working_dir,
            exclude_builtin_skills: false,
            session_manager: context.session_manager,
        })
    }

    /// Controls whether Goose's bundled skills are exposed by this client.
    /// Bundled skills are enabled by default.
    pub fn with_builtin_skills(mut self, enabled: bool) -> Self {
        self.exclude_builtin_skills = !enabled;
        self
    }

    fn discover_skills(&self) -> Vec<SourceEntry> {
        discover_skills(Some(&self.working_dir))
            .into_iter()
            .filter(|skill| {
                !self.exclude_builtin_skills || skill.source_type != SourceType::BuiltinSkill
            })
            .collect()
    }
}

/// A supporting file is cut at the tool-response limit so one large file cannot fill a local model's
/// window; the cut is announced in the result rather than hidden.
fn bound_supporting_file(content: &str, max_chars: usize) -> (String, Option<String>) {
    let total = content.chars().count();
    if total <= max_chars {
        return (content.to_string(), None);
    }
    let kept: String = content.chars().take(max_chars).collect();
    (
        kept,
        Some(format!(
            "TRUNCATED: showing {max_chars} of {total} characters (GOOSE_MAX_TOOL_RESPONSE_SIZE); read the rest with a file tool.\n"
        )),
    )
}

/// How far back the model can still read `rendered`, byte for byte, as a load_skill result, counted
/// in tool results: 1 is the most recent result in the conversation. `None` when no agent-visible
/// result carries it — never loaded, condensed or compacted away (both hide the original from the
/// agent), offloaded by the large-response handler, or rendered from a skill that has changed on
/// disk since.
fn tool_results_back(messages: &[Message], rendered: &str) -> Option<usize> {
    let results: Vec<_> = messages
        .iter()
        .filter(|m| m.is_agent_visible())
        .flat_map(|m| m.content.iter())
        .filter_map(|content| match content {
            MessageContent::ToolResponse(response) => Some(response),
            _ => None,
        })
        .collect();
    let at = results.iter().rposition(|response| {
        response.tool_result.as_ref().is_ok_and(|result| {
            result
                .content
                .iter()
                .any(|item| item.as_text().is_some_and(|text| text.text == rendered))
        })
    })?;
    Some(results.len() - at)
}

fn already_loaded_note(what: &str, chars: usize, back: usize) -> String {
    let where_it_is = if back == 1 {
        "your previous tool result".to_string()
    } else {
        format!("the load_skill result {back} tool results back")
    };
    format!(
        "{what} is already loaded in this conversation: {where_it_is} holds its full text ({chars} \
         characters), identical to what this call would return, so it was not sent again. Read and \
         follow it there — loading a skill only puts its text in your context; it does not run \
         anything or do the work. If compaction removes that earlier copy, or the skill changes on \
         disk, load_skill returns the full text again."
    )
}

impl SkillsClient {
    /// Q-297 (E2E #3o, session 20260928_17): the model loaded atlassian-migration-scripts-skill
    /// (55,278 chars, ~14.9k tokens) five times, four inside one turn — "Let me run the skill that
    /// matches this instead of doing it by hand" — and the context went 102k → 193k of 262k. A load
    /// whose exact text the model can still read in this conversation answers with a note naming
    /// where; anything else — the copy compacted away, the skill edited, the history unreadable —
    /// loads in full.
    async fn unless_already_loaded(
        &self,
        ctx: &ToolCallContext,
        what: &str,
        rendered: String,
    ) -> CallToolResult {
        let back = match self
            .session_manager
            .get_session(&ctx.session_id, true)
            .await
        {
            Ok(session) => session
                .conversation
                .as_ref()
                .and_then(|conversation| tool_results_back(conversation.messages(), &rendered)),
            Err(err) => {
                tracing::warn!(
                    session_id = %ctx.session_id,
                    %err,
                    "load_skill: session history unreadable, so {what} is sent in full without checking for an earlier copy"
                );
                None
            }
        };
        match back {
            Some(back) => {
                tracing::info!(%what, back, chars = rendered.len(), "skill already in the conversation; not sent again");
                CallToolResult::success(vec![Content::text(already_loaded_note(
                    what,
                    rendered.chars().count(),
                    back,
                ))])
            }
            None => CallToolResult::success(vec![Content::text(rendered)]),
        }
    }
}

#[async_trait]
impl McpClientTrait for SkillsClient {
    async fn list_tools(
        &self,
        _session_id: &str,
        _next_cursor: Option<String>,
        _cancellation_token: CancellationToken,
    ) -> Result<ListToolsResult, Error> {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the skill to load. Use \"skill-name/path\" to load a supporting file."
                },
                "args": {
                    "type": "string",
                    "description": "Optional arguments to provide when loading the skill."
                }
            }
        });

        let tool = Tool::new(
            "load_skill",
            "Load a skill's full content into your context so you can follow its instructions. \
             Loading returns the skill's text; it does not run the skill or do its work.\n\n\
             Skills are listed in your system instructions. When you need to use one, \
             load it first to get the detailed instructions. A loaded skill stays in this \
             conversation: follow it from that earlier result. Loading it again while that copy is \
             still in your context returns a short note instead of the text.\n\n\
             Examples:\n\
             - load_skill(name: \"gdrive\") → Loads the gdrive skill instructions\n\
             - load_skill(name: \"my-skill\", args: \"the arguments for the skill\") → Loads a skill with arguments\n\
             - load_skill(name: \"my-skill/template.md\") → Loads a supporting file"
                .to_string(),
            schema.as_object().unwrap().clone(),
        );

        Ok(ListToolsResult {
            tools: vec![tool],
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
        if name != "load_skill" {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Unknown tool: {}",
                name
            ))]));
        }

        let skill_name = arguments
            .as_ref()
            .and_then(|args| args.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if skill_name.is_empty() {
            return Ok(CallToolResult::error(vec![Content::text(
                "Missing required parameter: name",
            )]));
        }
        let args = arguments
            .as_ref()
            .and_then(|args| args.get("args"))
            .and_then(|v| v.as_str());

        let skills = self.discover_skills();

        if let Some(skill) = skills.iter().find(|s| s.name == skill_name) {
            tracing::info!(skill = %skill.name, chars = skill.content.len(), "skill loaded");
            return match loaded_skill_context_with_args(skill, args) {
                Ok(rendered) => Ok(self
                    .unless_already_loaded(ctx, &format!("Skill '{}'", skill.name), rendered)
                    .await),
                Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to parse skill arguments: {}",
                    e
                ))])),
            };
        }

        if let Some((parent_skill_name, raw_relative_path)) = skill_name.split_once('/') {
            let relative_path = raw_relative_path.replace('\\', "/");
            if let Some(skill) = skills.iter().find(|s| {
                s.name == parent_skill_name
                    && matches!(s.source_type, SourceType::Skill | SourceType::BuiltinSkill)
            }) {
                let skill_dir = PathBuf::from(&skill.path);
                let canonical_skill_dir = skill_dir
                    .canonicalize()
                    .unwrap_or_else(|_| skill_dir.clone());

                for file_path in &skill.supporting_files {
                    let file_path_buf = Path::new(file_path);
                    let Ok(rel) = file_path_buf.strip_prefix(&skill_dir) else {
                        continue;
                    };
                    if rel.to_string_lossy().replace('\\', "/") != relative_path {
                        continue;
                    }

                    return Ok(match file_path_buf.canonicalize() {
                        Ok(canonical) if canonical.starts_with(&canonical_skill_dir) => {
                            match std::fs::read_to_string(&canonical) {
                                Ok(content) => {
                                    let (content, note) = bound_supporting_file(
                                        &content,
                                        crate::agents::large_response_handler::large_text_threshold(
                                        ),
                                    );
                                    tracing::info!(file = %skill_name, chars = content.len(), truncated = note.is_some(), "skill file loaded");
                                    let rendered = format!(
                                        "# Loaded: {}\n\n{}\n\n---\n{}File loaded into context.",
                                        skill_name,
                                        content,
                                        note.unwrap_or_default()
                                    );
                                    self.unless_already_loaded(
                                        ctx,
                                        &format!("File '{skill_name}'"),
                                        rendered,
                                    )
                                    .await
                                }
                                Err(e) => CallToolResult::error(vec![Content::text(format!(
                                    "Failed to read '{}': {}",
                                    skill_name, e
                                ))]),
                            }
                        }
                        Ok(_) => CallToolResult::error(vec![Content::text(format!(
                            "Refusing to load '{}': resolves outside the skill directory",
                            skill_name
                        ))]),
                        Err(e) => CallToolResult::error(vec![Content::text(format!(
                            "Failed to resolve '{}': {}",
                            skill_name, e
                        ))]),
                    });
                }

                let available: Vec<String> = skill
                    .supporting_files
                    .iter()
                    .filter_map(|f| {
                        Path::new(f)
                            .strip_prefix(&skill_dir)
                            .ok()
                            .map(|r| r.to_string_lossy().replace('\\', "/"))
                    })
                    .take(10)
                    .collect();

                return Ok(if available.is_empty() {
                    CallToolResult::error(vec![Content::text(format!(
                        "Skill '{}' has no supporting files.",
                        skill.name
                    ))])
                } else {
                    CallToolResult::error(vec![Content::text(format!(
                        "File '{}' not found. Available: {}",
                        skill_name,
                        available.join(", ")
                    ))])
                });
            }
        }

        let suggestions: Vec<&str> = skills
            .iter()
            .filter(|s| {
                s.name.to_lowercase().contains(&skill_name.to_lowercase())
                    || skill_name.to_lowercase().contains(&s.name.to_lowercase())
            })
            .take(3)
            .map(|s| s.name.as_str())
            .collect();

        Ok(if suggestions.is_empty() {
            CallToolResult::error(vec![Content::text(format!(
                "Skill '{}' not found.",
                skill_name
            ))])
        } else {
            CallToolResult::error(vec![Content::text(format!(
                "Skill '{}' not found. Did you mean: {}?",
                skill_name,
                suggestions.join(", ")
            ))])
        })
    }

    fn get_info(&self) -> Option<&InitializeResult> {
        Some(&self.info)
    }

    fn get_instructions(&self) -> Option<String> {
        let sources = self.discover_skills();
        let mut skills: Vec<&SourceEntry> = sources
            .iter()
            .filter(|s| {
                s.source_type == SourceType::Skill || s.source_type == SourceType::BuiltinSkill
            })
            .collect();
        skills.sort_by(|a, b| (&a.name, &a.path).cmp(&(&b.name, &b.path)));

        if skills.is_empty() {
            return None;
        }

        let mut instructions = String::from(
            "\n\nYou have these skills at your disposal, when it is clear they can help you solve a problem or you are asked to use them:",
        );
        for skill in &skills {
            instructions.push_str(&format!("\n• {} - {}", skill.name, skill.description));
        }
        Some(instructions)
    }

    async fn subscribe(&self) -> mpsc::Receiver<ServerNotification> {
        let (_tx, rx) = mpsc::channel(1);
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_load_filesystem_skill_without_builtin_skills() {
        let temp_dir = TempDir::new().unwrap();
        let skill_dir = temp_dir.path().join(".goose/skills/my-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: A test skill\n---\nDo the thing.",
        )
        .unwrap();

        let session = std::sync::Arc::new(crate::session::Session {
            working_dir: temp_dir.path().to_path_buf(),
            ..crate::session::Session::default()
        });
        let client = SkillsClient::new(PlatformExtensionContext {
            extension_manager: None,
            session_manager: Arc::new(crate::session::SessionManager::instance()),
            session: Some(session),
            use_login_shell_path: false,
            working_dir: None,
        })
        .unwrap()
        .with_builtin_skills(false);

        assert!(client
            .discover_skills()
            .iter()
            .all(|skill| skill.source_type != SourceType::BuiltinSkill));

        let ctx = ToolCallContext::new("test".to_string(), None, None);
        let args: JsonObject =
            serde_json::from_value(serde_json::json!({"name": "my-skill"})).unwrap();
        let result = client
            .call_tool(&ctx, "load_skill", Some(args), CancellationToken::new())
            .await
            .unwrap();

        assert!(!result.is_error.unwrap_or(false));
        let text = match &result.content[0].raw {
            rmcp::model::RawContent::Text(t) => &t.text,
            _ => panic!("expected text"),
        };
        assert!(text.contains("my-skill"));
        assert!(text.contains("Do the thing"));
    }

    #[tokio::test]
    async fn test_load_skill_not_found_returns_error() {
        let temp_dir = TempDir::new().unwrap();
        let client = SkillsClient::new(PlatformExtensionContext {
            extension_manager: None,
            session_manager: Arc::new(crate::session::SessionManager::instance()),
            session: None,
            use_login_shell_path: false,
            working_dir: Some(temp_dir.path().to_path_buf()),
        })
        .unwrap();

        let ctx = ToolCallContext::new("test".to_string(), None, None);
        let args: JsonObject =
            serde_json::from_value(serde_json::json!({"name": "nonexistent"})).unwrap();
        let result = client
            .call_tool(&ctx, "load_skill", Some(args), CancellationToken::new())
            .await
            .unwrap();

        assert!(result.is_error.unwrap_or(false));
    }

    fn text_of(result: &CallToolResult) -> String {
        match &result.content[0].raw {
            rmcp::model::RawContent::Text(t) => t.text.clone(),
            _ => panic!("expected text"),
        }
    }

    /// A session in its own store holding one skill with a supporting file, and a client on it.
    async fn session_with_skill(
        temp_dir: &TempDir,
    ) -> (SkillsClient, Arc<SessionManager>, String, PathBuf) {
        let skill_dir = temp_dir.path().join("project/.agents/skills/big-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: big-skill\ndescription: A large skill\n---\nStep one, step two.",
        )
        .unwrap();
        fs::write(skill_dir.join("template.md"), "A template.").unwrap();

        let session_manager = Arc::new(SessionManager::new(temp_dir.path().join("sessions")));
        let session = session_manager
            .create_session(
                temp_dir.path().join("project"),
                "q-297".to_string(),
                crate::session::session_manager::SessionType::User,
                crate::config::GooseMode::Auto,
            )
            .await
            .unwrap();
        let client = SkillsClient::new(PlatformExtensionContext {
            extension_manager: None,
            session_manager: session_manager.clone(),
            session: None,
            use_login_shell_path: false,
            working_dir: Some(temp_dir.path().join("project")),
        })
        .unwrap()
        .with_builtin_skills(false);
        (client, session_manager, session.id, skill_dir)
    }

    /// Runs load_skill(name) as the agent loop does: the call, then the request and its result
    /// saved to the session.
    async fn load(
        client: &SkillsClient,
        session_manager: &SessionManager,
        session_id: &str,
        name: &str,
    ) -> String {
        let args: JsonObject = serde_json::from_value(serde_json::json!({ "name": name })).unwrap();
        let ctx = ToolCallContext::new(session_id.to_string(), None, None);
        let result = client
            .call_tool(
                &ctx,
                "load_skill",
                Some(args.clone()),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(!result.is_error.unwrap_or(false));
        let id = format!("call-{}", uuid::Uuid::new_v4());
        session_manager
            .add_message(
                session_id,
                &Message::assistant().with_tool_request(
                    id.clone(),
                    Ok(rmcp::model::CallToolRequestParams::new("load_skill").with_arguments(args)),
                ),
            )
            .await
            .unwrap();
        session_manager
            .add_message(
                session_id,
                &Message::user().with_tool_response(id, Ok(result.clone())),
            )
            .await
            .unwrap();
        text_of(&result)
    }

    /// Q-297: the second load of an unchanged skill in one conversation names the earlier copy
    /// instead of sending the text again; so does a supporting file's.
    #[tokio::test]
    async fn a_skill_already_in_the_conversation_is_not_sent_again() {
        let temp_dir = TempDir::new().unwrap();
        let (client, sessions, session_id, _) = session_with_skill(&temp_dir).await;

        let first = load(&client, &sessions, &session_id, "big-skill").await;
        assert!(first.starts_with("# Loaded Skill: big-skill"));
        assert!(first.contains("Step one, step two."));

        let second = load(&client, &sessions, &session_id, "big-skill").await;
        assert!(!second.contains("Step one, step two."), "{second}");
        assert!(
            second.starts_with("Skill 'big-skill' is already loaded in this conversation"),
            "{second}"
        );
        assert!(
            second.contains("your previous tool result holds"),
            "{second}"
        );
        assert!(second.contains(&format!("({} characters)", first.chars().count())));

        let file = load(&client, &sessions, &session_id, "big-skill/template.md").await;
        assert!(file.contains("A template."));
        let file_again = load(&client, &sessions, &session_id, "big-skill/template.md").await;
        assert!(
            file_again.starts_with("File 'big-skill/template.md' is already loaded"),
            "{file_again}"
        );

        let third = load(&client, &sessions, &session_id, "big-skill").await;
        assert!(
            third.contains("the load_skill result 4 tool results back holds"),
            "{third}"
        );
    }

    /// Q-297: once compaction hides the earlier copy from the model, the skill loads in full.
    #[tokio::test]
    async fn a_skill_compacted_out_of_the_conversation_loads_in_full() {
        let temp_dir = TempDir::new().unwrap();
        let (client, sessions, session_id, _) = session_with_skill(&temp_dir).await;
        load(&client, &sessions, &session_id, "big-skill").await;

        let session = sessions.get_session(&session_id, true).await.unwrap();
        let compacted = crate::conversation::Conversation::new_unvalidated(
            session
                .conversation
                .unwrap()
                .messages()
                .iter()
                .map(|m| m.clone().with_visibility(true, false)),
        );
        sessions
            .replace_conversation(&session_id, &compacted)
            .await
            .unwrap();

        let again = load(&client, &sessions, &session_id, "big-skill").await;
        assert!(again.starts_with("# Loaded Skill: big-skill"), "{again}");
        assert!(again.contains("Step one, step two."));
    }

    /// Q-297: a skill edited on disk since its earlier load is new text, so it loads in full.
    #[tokio::test]
    async fn a_skill_changed_on_disk_loads_in_full() {
        let temp_dir = TempDir::new().unwrap();
        let (client, sessions, session_id, skill_dir) = session_with_skill(&temp_dir).await;
        load(&client, &sessions, &session_id, "big-skill").await;

        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: big-skill\ndescription: A large skill\n---\nStep one, step two, step three.",
        )
        .unwrap();

        let again = load(&client, &sessions, &session_id, "big-skill").await;
        assert!(again.contains("Step one, step two, step three."), "{again}");
    }

    /// Q-518: the listing and load_skill answer from remembered walks of the skill tree; a skill
    /// added or edited between two calls, a supporting file added, and a supporting file's new text
    /// are what the next call sees.
    #[tokio::test]
    async fn a_skill_changed_between_two_calls_is_what_the_next_call_sees() {
        let temp_dir = TempDir::new().unwrap();
        let (client, sessions, session_id, skill_dir) = session_with_skill(&temp_dir).await;
        let listing = client.get_instructions().unwrap();
        assert!(listing.contains("big-skill - A large skill"), "{listing}");
        let file = load(&client, &sessions, &session_id, "big-skill/template.md").await;
        assert!(file.contains("A template."), "{file}");

        fs::write(skill_dir.join("template.md"), "A new template.").unwrap();
        let file = load(&client, &sessions, &session_id, "big-skill/template.md").await;
        assert!(file.contains("A new template."), "{file}");

        let late = skill_dir.with_file_name("late-skill");
        fs::create_dir_all(&late).unwrap();
        fs::write(
            late.join("SKILL.md"),
            "---\nname: late-skill\ndescription: Added between calls\n---\nLate.",
        )
        .unwrap();
        let listing = client.get_instructions().unwrap();
        assert!(
            listing.contains("late-skill - Added between calls"),
            "{listing}"
        );

        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: big-skill\ndescription: A changed purpose\n---\nStep three.",
        )
        .unwrap();
        let listing = client.get_instructions().unwrap();
        assert!(
            listing.contains("big-skill - A changed purpose"),
            "{listing}"
        );

        fs::write(skill_dir.join("extra.md"), "An extra file.").unwrap();
        let extra = load(&client, &sessions, &session_id, "big-skill/extra.md").await;
        assert!(extra.contains("An extra file."), "{extra}");
    }

    /// Q-297: a history goose cannot read proves nothing about an earlier copy — the skill loads
    /// in full.
    #[tokio::test]
    async fn an_unreadable_history_loads_the_skill_in_full() {
        let temp_dir = TempDir::new().unwrap();
        let (client, _, _, _) = session_with_skill(&temp_dir).await;
        let args: JsonObject =
            serde_json::from_value(serde_json::json!({"name": "big-skill"})).unwrap();
        let ctx = ToolCallContext::new("no-such-session".to_string(), None, None);
        let result = client
            .call_tool(&ctx, "load_skill", Some(args), CancellationToken::new())
            .await
            .unwrap();
        assert!(text_of(&result).contains("Step one, step two."));
    }

    /// Q-267: the skills client reads the folder the extension manager started it for — a chat's
    /// project skills — and with no folder at all it refuses instead of reading goosed's cwd.
    #[tokio::test]
    async fn the_skills_client_reads_the_folder_it_was_started_for() {
        let temp_dir = TempDir::new().unwrap();
        let skill_dir = temp_dir.path().join(".agents/skills/project-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: project-skill\ndescription: The project's own skill\n---\nDo it.",
        )
        .unwrap();
        let context = |working_dir: Option<std::path::PathBuf>| PlatformExtensionContext {
            extension_manager: None,
            session_manager: Arc::new(crate::session::SessionManager::instance()),
            session: None,
            use_login_shell_path: false,
            working_dir,
        };

        let client = SkillsClient::new(context(Some(temp_dir.path().to_path_buf()))).unwrap();
        assert!(client
            .discover_skills()
            .iter()
            .any(|skill| skill.name == "project-skill"));

        assert!(SkillsClient::new(context(None)).is_err());
    }
}
