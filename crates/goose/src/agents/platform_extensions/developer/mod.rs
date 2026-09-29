pub mod edit;
pub mod file_diff;
pub mod image;
pub mod process_groups;
pub mod shell;
pub mod shell_watchdog;
mod stall;
pub mod tree;

use crate::agents::extension::PlatformExtensionContext;
use crate::agents::mcp_client::{Error, McpClientTrait};
use crate::agents::ToolCallContext;
use anyhow::Result;
use async_trait::async_trait;
use edit::{EditTools, FileEditParams, FileWriteParams};
use image::{ImageReadParams, ImageTool};
use indoc::indoc;
use rmcp::model::{
    CallToolResult, Content, Implementation, InitializeResult, JsonObject, ListToolsResult,
    ServerCapabilities, Tool, ToolAnnotations,
};
use schemars::{schema_for, JsonSchema};
use serde_json::Value;
use shell::{shell_display_name, ShellOutput, ShellParams, ShellTool};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tree::{TreeParams, TreeTool};

pub static EXTENSION_NAME: &str = "developer";

pub struct DeveloperClient {
    info: InitializeResult,
    shell_tool: Arc<ShellTool>,
    edit_tools: Arc<EditTools>,
    tree_tool: Arc<TreeTool>,
    image_tool: Arc<ImageTool>,
}

fn developer_instructions() -> &'static str {
    if cfg!(windows) {
        indoc! {"
            Use the developer extension to build software and operate a terminal.

            Make sure to use the tools *efficiently* - reading all the content you need in as few
            iterations as possible and then making the requested edits or running commands. You are
            responsible for managing your context window, and to minimize unnecessary turns which
            cost the user money.

            For editing software, prefer the flow of using tree to understand the codebase structure
            and file sizes. When you need to search, prefer findstr or Select-String (via shell).
            Then use type or Get-Content to gather the context you need, always reading before
            editing. Use write and edit to efficiently make changes. Test and verify as appropriate.
        "}
    } else {
        indoc! {"
            Use the developer extension to build software and operate a terminal.

            Make sure to use the tools *efficiently* - reading all the content you need in as few
            iterations as possible and then making the requested edits or running commands. You are
            responsible for managing your context window, and to minimize unnecessary turns which
            cost the user money.

            For editing software, prefer the flow of using tree to understand the codebase structure
            and file sizes. When you need to search, prefer rg which correctly respects gitignored
            content. Then use cat or sed to gather the context you need, always reading before editing.
            Use write and edit to efficiently make changes. Test and verify as appropriate.

            When running Python scripts or commands, always use `python3` instead of `python`.

            On macOS, find files by name with `mdfind -onlyin <dir> -name <name>` instead of walking $HOME with find: a privacy-protected folder there can block the walk until the command times out.
        "}
    }
}

impl DeveloperClient {
    pub fn new(context: PlatformExtensionContext) -> Result<Self> {
        let info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(EXTENSION_NAME, "1.0.0").with_title("Developer"))
            .with_instructions(developer_instructions());

        Ok(Self {
            info,
            shell_tool: Arc::new(ShellTool::new(context.use_login_shell_path)?),
            edit_tools: Arc::new(EditTools::new()),
            tree_tool: Arc::new(TreeTool::new()),
            image_tool: Arc::new(ImageTool::new()),
        })
    }

    fn schema<T: JsonSchema>() -> JsonObject {
        serde_json::to_value(schema_for!(T))
            .expect("schema serialization should succeed")
            .as_object()
            .expect("schema should serialize to an object")
            .clone()
    }

    /// Parses a tool call's arguments. When required fields are absent the answer names them,
    /// lists the field NAMES that did arrive (never their values, which can be whole files) and
    /// says how to retry — Q-482: a bare serde "missing field `path`" was misread by local models
    /// as a match failure and answered with a whole-file rewrite. The path is never guessed.
    pub fn parse_args<T: serde::de::DeserializeOwned + JsonSchema>(
        tool: &str,
        arguments: Option<JsonObject>,
    ) -> Result<T, String> {
        let arguments = arguments.unwrap_or_default();
        let required = Self::required_fields::<T>();
        let missing: Vec<&str> = required
            .iter()
            .map(String::as_str)
            .filter(|field| arguments.get(*field).is_none_or(Value::is_null))
            .collect();
        if !missing.is_empty() {
            return Err(missing_fields_message(
                tool, &required, &missing, &arguments,
            ));
        }
        serde_json::from_value(Value::Object(arguments))
            .map_err(|e| format!("Failed to parse arguments: {e}"))
    }

    fn required_fields<T: JsonSchema>() -> Vec<String> {
        Self::schema::<T>()
            .get("required")
            .and_then(Value::as_array)
            .map(|fields| {
                fields
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn get_tools() -> Vec<Tool> {
        vec![
            Tool::new(
                "write".to_string(),
                "Create a new file or overwrite an existing file. Creates parent directories if needed.".to_string(),
                Self::schema::<FileWriteParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Write".to_string()),
                Some(false),
                Some(true),
                Some(false),
                Some(false),
            )),
            Tool::new(
                "edit".to_string(),
                "Edit a file by finding and replacing text. The before text must match exactly and uniquely. Use empty after text to delete.".to_string(),
                Self::schema::<FileEditParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Edit".to_string()),
                Some(false),
                Some(true),
                Some(false),
                Some(false),
            )),
            Tool::new(
                "shell".to_string(),
                format!(
                    "Execute a shell command in the current dir. Commands run under `{shell}` \
                     (set GOOSE_SHELL to override) - write command strings in that shell's \
                     syntax. Returns an object with stdout and stderr as separate fields. The \
                     output of each stream is limited to up to 2000 lines, and longer outputs \
                     will be saved to a temporary file.",
                    shell = shell_display_name(),
                ),
                Self::schema::<ShellParams>(),
            )
            .with_output_schema::<ShellOutput>()
            .annotate(ToolAnnotations::from_raw(
                Some("Shell".to_string()),
                Some(false),
                Some(true),
                Some(false),
                Some(true),
            )),
            Tool::new(
                "tree".to_string(),
                "List a directory tree with line counts. Traversal respects .gitignore rules.".to_string(),
                Self::schema::<TreeParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Tree".to_string()),
                Some(true),
                Some(false),
                Some(true),
                Some(false),
            )),
            Tool::new(
                "read_image".to_string(),
                "Read an image from a local file path or http(s) URL and return it as image content for the model to inspect. Supports png, jpeg, gif, and webp.".to_string(),
                Self::schema::<ImageReadParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Read Image".to_string()),
                Some(true),
                Some(false),
                Some(true),
                Some(false),
            )),
        ]
    }
}

fn backticked(fields: &[&str]) -> String {
    fields
        .iter()
        .map(|field| format!("`{field}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn spoken_list(fields: &[&str]) -> String {
    match fields {
        [] => String::new(),
        [only] => (*only).to_string(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

fn missing_fields_message(
    tool: &str,
    required: &[String],
    missing: &[&str],
    arguments: &JsonObject,
) -> String {
    let noun = if missing.len() == 1 {
        "field"
    } else {
        "fields"
    };
    // Schema order first, then extras by name: the map's own order depends on whether
    // serde_json's preserve_order feature is unified into the build.
    let mut arrived: Vec<&str> = arguments
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, _)| key.as_str())
        .collect();
    arrived.sort_by_key(|key| {
        (
            required
                .iter()
                .position(|field| field == key)
                .unwrap_or(required.len()),
            *key,
        )
    });
    let arrived = if arrived.is_empty() {
        "no fields".to_string()
    } else {
        backticked(&arrived)
    };
    let rest: Vec<&str> = required
        .iter()
        .map(String::as_str)
        .filter(|field| !missing.contains(field))
        .collect();
    let retry = if rest.is_empty() {
        format!("Call {tool} again with {}.", spoken_list(missing))
    } else {
        format!(
            "Call {tool} again with {} first, then {}.",
            spoken_list(missing),
            spoken_list(&rest)
        )
    };
    format!(
        "Failed to parse arguments: missing required {noun} {}. Arrived: {arrived}. Nothing was changed. {retry}",
        backticked(missing)
    )
}

#[async_trait]
impl McpClientTrait for DeveloperClient {
    async fn list_tools(
        &self,
        _session_id: &str,
        _next_cursor: Option<String>,
        _cancellation_token: CancellationToken,
    ) -> Result<ListToolsResult, Error> {
        Ok(ListToolsResult {
            tools: Self::get_tools(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        ctx: &ToolCallContext,
        name: &str,
        arguments: Option<JsonObject>,
        cancel_token: CancellationToken,
    ) -> Result<CallToolResult, Error> {
        let working_dir = ctx.working_dir.as_deref();
        match name {
            "shell" => match Self::parse_args::<ShellParams>(name, arguments) {
                // The session id rides along so an own-group spawn is registered under the session
                // that made it — the attempt-scoped reap (see process_groups) keys on it.
                // A cancelled call drops the shell future, and dropping it terminates the
                // command's processes (process_groups::CommandProcesses) instead of leaving them.
                Ok(params) => tokio::select! {
                    result = self
                        .shell_tool
                        .shell_in_session(params, working_dir, Some(&ctx.session_id)) => Ok(result),
                    _ = cancel_token.cancelled() => Ok(ShellTool::error_result(
                        "Shell command cancelled; its processes were terminated.",
                        None,
                    )),
                },
                Err(error) => Ok(ShellTool::error_result(&format!("Error: {error}"), None)),
            },
            "write" => match Self::parse_args::<FileWriteParams>(name, arguments) {
                Ok(params) => Ok(self.edit_tools.file_write_with_cwd(params, working_dir)),
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            "edit" => match Self::parse_args::<FileEditParams>(name, arguments) {
                Ok(params) => Ok(self.edit_tools.file_edit_with_cwd(params, working_dir)),
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            "tree" => match Self::parse_args::<TreeParams>(name, arguments) {
                Ok(params) => Ok(self.tree_tool.tree_with_cwd(params, working_dir)),
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            "read_image" => match Self::parse_args::<ImageReadParams>(name, arguments) {
                Ok(params) => Ok(self
                    .image_tool
                    .image_read_with_cwd(params, working_dir)
                    .await),
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            _ => Ok(CallToolResult::error(vec![Content::text(format!(
                "Error: Unknown tool: {name}"
            ))
            .with_priority(0.0)])),
        }
    }

    fn get_info(&self) -> Option<&InitializeResult> {
        Some(&self.info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionManager;
    use rmcp::model::RawContent;
    use rmcp::object;
    use std::fs;

    #[test]
    fn developer_tools_are_flat() {
        let names: Vec<String> = DeveloperClient::get_tools()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();

        assert_eq!(names, vec!["write", "edit", "shell", "tree", "read_image"]);
    }

    fn test_context(data_dir: std::path::PathBuf) -> PlatformExtensionContext {
        PlatformExtensionContext {
            extension_manager: None,
            session_manager: Arc::new(SessionManager::new(data_dir)),
            session: None,
            use_login_shell_path: false,
            working_dir: None,
        }
    }

    fn first_text(result: &CallToolResult) -> &str {
        match &result.content[0].raw {
            RawContent::Text(text) => &text.text,
            _ => panic!("expected text content"),
        }
    }

    #[tokio::test]
    async fn developer_client_uses_working_dir_for_file_tools() {
        let temp = tempfile::tempdir().unwrap();
        let client = DeveloperClient::new(test_context(temp.path().join("sessions"))).unwrap();
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).unwrap();

        let ctx = ToolCallContext::new("session".to_owned(), Some(cwd.clone()), None);
        let write = client
            .call_tool(
                &ctx,
                "write",
                Some(object!({
                    "path": "notes.txt",
                    "content": "first line"
                })),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(write.is_error, Some(false), "{}", first_text(&write));
        assert_eq!(
            fs::read_to_string(cwd.join("notes.txt")).unwrap(),
            "first line"
        );

        let edit = client
            .call_tool(
                &ctx,
                "edit",
                Some(object!({
                    "path": "notes.txt",
                    "before": "first",
                    "after": "updated"
                })),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(edit.is_error, Some(false), "{}", first_text(&edit));
        assert_eq!(
            fs::read_to_string(cwd.join("notes.txt")).unwrap(),
            "updated line"
        );
    }

    #[tokio::test]
    async fn edit_without_path_names_the_field_and_changes_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let client = DeveloperClient::new(test_context(temp.path().join("sessions"))).unwrap();
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).unwrap();
        fs::write(cwd.join("notes.txt"), "first line").unwrap();
        let huge_before = format!("first{}", "BEFORE-VALUE-".repeat(4096));

        let ctx = ToolCallContext::new("session".to_owned(), Some(cwd.clone()), None);
        let edit = client
            .call_tool(
                &ctx,
                "edit",
                Some(object!({
                    "before": huge_before,
                    "after": "updated"
                })),
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(edit.is_error, Some(true));
        assert_eq!(
            first_text(&edit),
            "Error: Failed to parse arguments: missing required field `path`. \
             Arrived: `before`, `after`. Nothing was changed. \
             Call edit again with path first, then before and after."
        );
        assert!(!first_text(&edit).contains("BEFORE-VALUE-"));
        assert!(!first_text(&edit).contains("updated"));
        assert_eq!(
            fs::read_to_string(cwd.join("notes.txt")).unwrap(),
            "first line"
        );
    }

    #[tokio::test]
    async fn every_path_tool_answers_a_missing_path_with_the_retry() {
        let temp = tempfile::tempdir().unwrap();
        let client = DeveloperClient::new(test_context(temp.path().join("sessions"))).unwrap();
        let ctx = ToolCallContext::new("session".to_owned(), Some(temp.path().into()), None);
        let mut checked = Vec::new();

        for tool in DeveloperClient::get_tools() {
            let required: Vec<String> = tool
                .input_schema
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            if !required.iter().any(|field| field == "path") {
                continue;
            }
            let mut arguments = JsonObject::new();
            for field in required.iter().filter(|field| *field != "path") {
                arguments.insert(field.clone(), Value::String("x".into()));
            }
            let result = client
                .call_tool(&ctx, &tool.name, Some(arguments), CancellationToken::new())
                .await
                .unwrap();
            let text = first_text(&result);
            assert_eq!(result.is_error, Some(true), "{}: {text}", tool.name);
            assert!(
                text.contains("missing required field `path`")
                    && text.contains("Nothing was changed.")
                    && text.contains(&format!("Call {} again with path", tool.name)),
                "{}: {text}",
                tool.name
            );
            checked.push(tool.name.to_string());
        }

        assert_eq!(checked, vec!["write", "edit", "tree"]);
    }

    #[test]
    fn missing_arguments_list_every_required_field_and_no_arrivals() {
        let error = DeveloperClient::parse_args::<FileWriteParams>("write", None).unwrap_err();
        assert_eq!(
            error,
            "Failed to parse arguments: missing required fields `path`, `content`. \
             Arrived: no fields. Nothing was changed. Call write again with path and content."
        );
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn developer_client_uses_working_dir_for_shell_tool() {
        let temp = tempfile::tempdir().unwrap();
        let client = DeveloperClient::new(test_context(temp.path().join("sessions"))).unwrap();
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).unwrap();

        let ctx = ToolCallContext::new("session".to_owned(), Some(cwd.clone()), None);
        let result = client
            .call_tool(
                &ctx,
                "shell",
                Some(object!({
                    "command": "pwd"
                })),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(false), "{}", first_text(&result));
        let observed = std::fs::canonicalize(first_text(&result)).unwrap();
        let expected = std::fs::canonicalize(&cwd).unwrap();
        assert_eq!(observed, expected);
    }
}
