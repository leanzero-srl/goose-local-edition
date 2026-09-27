//! The `loop` platform extension: one tool, `loop_report`, that ends a session loop's tick (design
//! local-edition/mlx/quality/DESIGN-SESSION-LOOPS.md §4.4, §9 L3; Q-228).
//!
//! The agent carries it only while its chat has a loop that has not ended
//! (`session_loops::agent_sync`), so a chat without a loop never sees the tool, and a loop chat's
//! tool list does not change between ticks and user turns. A valid call writes the report onto the
//! tick in flight through the record's one read-modify-write door and ENDS the turn (the
//! `ask_user` mechanism, `END_TURN_META_KEY`). A refused call does not end the turn, so the model
//! can call it again with the field fixed. A tick that never calls it is recorded `no_report` by
//! the runner; nothing here infers a verdict from prose.
//!
//! The schema is flat and every argument is a plain `"type": "string"`. The local engines parse
//! the 27B/Flash's qwen3_coder calls with mlx_lm's `_convert_param_value`: a property whose type
//! is not a plain string (schemars' `["string", "null"]` for an `Option<String>`) falls through to
//! `ast.literal_eval`, which refuses a word like `10m` and drops the whole call. The model's
//! literal `null` arrives as JSON null, which an absent optional also means.

use crate::agents::extension::PlatformExtensionContext;
use crate::agents::mcp_client::{Error, McpClientTrait};
use crate::agents::tool_execution::ToolCallContext;
use crate::loop_clock::parse_cadence;
use crate::needs_you::END_TURN_META_KEY;
use crate::session::SessionManager;
use crate::session_loops::record;
use anyhow::Result;
use async_trait::async_trait;
use goose_sdk_types::custom_requests::{LoopCadence, LoopReport, LoopVerdict};
use indoc::indoc;
use rmcp::model::{
    CallToolResult, Content, Implementation, InitializeResult, JsonObject, ListToolsResult, Meta,
    ServerCapabilities, Tool, ToolAnnotations,
};
use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;

pub static EXTENSION_NAME: &str = record::EXTENSION_NAME;
pub const LOOP_REPORT_TOOL_NAME: &str = "loop_report";

pub const NO_TICK: &str = "No loop tick is running in this chat.";
pub const RECORDED: &str = "Recorded. This tick ends now.";

const TOOL_DESCRIPTION: &str = indoc! {r#"
    Report what this loop tick did and end the tick. Call it once, as your last action, after
    you have rewritten the loop's state file. The tick ends when the report is recorded. If the
    report is refused, fix the field the refusal names and call it again.
"#};

/// The tool's input schema, exactly as the model sees it.
pub fn report_schema() -> JsonObject {
    let schema = json!({
        "type": "object",
        "properties": {
            "verdict": {
                "type": "string",
                "enum": ["progress", "done", "blocked"],
                "description": "progress: this tick moved the goal forward and there is more to do. done: the goal is met. blocked: only the user can decide what comes next (say what in blocked_on)."
            },
            "summary": {
                "type": "string",
                "description": "What this tick did, with its evidence: the command you ran and what it printed, or file:line."
            },
            "next_step": {
                "type": "string",
                "description": "The one concrete next step: which file, which command."
            },
            "next_in": {
                "type": "string",
                "description": "Only when the loop lets goose decide when: how long until the next tick, a number and s, m or h (\"10m\", \"2h\")."
            },
            "next_reason": {
                "type": "string",
                "description": "Why that wait."
            },
            "blocked_on": {
                "type": "string",
                "description": "Only with verdict blocked: the one thing only the user can decide."
            }
        },
        "required": ["verdict", "summary", "next_step"]
    });
    match schema {
        serde_json::Value::Object(object) => object,
        _ => unreachable!("the schema literal is an object"),
    }
}

/// Every argument as the model sent it. A JSON null and an absent key read the same.
#[derive(Debug, Default, Deserialize)]
struct ReportArgs {
    verdict: Option<String>,
    summary: Option<String>,
    next_step: Option<String>,
    next_in: Option<String>,
    next_reason: Option<String>,
    blocked_on: Option<String>,
}

fn given(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn parse_args(arguments: Option<JsonObject>) -> Result<ReportArgs, String> {
    let arguments = arguments.ok_or_else(|| {
        "loop_report needs its arguments: verdict, summary and next_step.".to_string()
    })?;
    serde_json::from_value(serde_json::Value::Object(arguments))
        .map_err(|e| format!("The arguments could not be read: {e}. Every argument is a string."))
}

/// The report the arguments make for a loop with this cadence, or the refusal naming the field.
/// A self-paced tick that names no delay is accepted: the loop then waits for the user, by name
/// (`waiting_you`); a delay it does name must be in the one cadence grammar.
fn validate(args: ReportArgs, cadence: &LoopCadence) -> Result<LoopReport, String> {
    let verdict_text = given(args.verdict)
        .ok_or_else(|| "verdict is missing: say progress, done or blocked.".to_string())?;
    let verdict = match verdict_text.to_lowercase().as_str() {
        "progress" => LoopVerdict::Progress,
        "done" => LoopVerdict::Done,
        "blocked" => LoopVerdict::Blocked,
        _ => {
            return Err(format!(
                "verdict \"{verdict_text}\" is not progress, done or blocked."
            ))
        }
    };
    let summary = given(args.summary).ok_or_else(|| {
        "summary is empty: say what this tick did, with its evidence.".to_string()
    })?;
    let next_step = given(args.next_step)
        .ok_or_else(|| "next_step is empty: name the one concrete next step.".to_string())?;
    let blocked_on = given(args.blocked_on);
    if verdict == LoopVerdict::Blocked && blocked_on.is_none() {
        return Err(
            "verdict is blocked but blocked_on is empty: say what only the user can decide."
                .to_string(),
        );
    }
    let next_in = given(args.next_in);
    if let (LoopCadence::SelfPaced, Some(delay)) = (cadence, &next_in) {
        if parse_cadence(delay).is_none() {
            return Err(format!(
                "next_in \"{delay}\" is not a number and s, m or h (\"10m\", \"2h\")."
            ));
        }
    }
    Ok(LoopReport {
        verdict,
        summary,
        next_step,
        next_in,
        next_reason: given(args.next_reason),
        blocked_on,
    })
}

/// A refusal the model reads and can act on; nothing was written.
#[derive(Debug)]
struct Refused(String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

/// Validate the call and write its report onto the tick in flight (the record's last tick, started
/// and not ended), replacing a report an earlier call of the same tick wrote. `Ok` = the tick's
/// number; `Err` = the words the model reads, with nothing written.
pub async fn record_report(
    session_manager: &SessionManager,
    session_id: &str,
    arguments: Option<JsonObject>,
) -> Result<u32, String> {
    let args = parse_args(arguments)?;
    let written = record::update(session_manager, session_id, |current| {
        let mut record = current.ok_or_else(|| Refused(NO_TICK.to_string()))?;
        let cadence = record.cadence.clone();
        let tick = record
            .ticks
            .last_mut()
            .filter(|tick| tick.ended_at.is_none())
            .ok_or_else(|| Refused(NO_TICK.to_string()))?;
        tick.report = Some(validate(args, &cadence).map_err(Refused)?);
        let n = tick.n;
        Ok((record, n))
    })
    .await;
    written.map_err(|e| match e.downcast_ref::<Refused>() {
        Some(refused) => refused.0.clone(),
        None => format!("The report could not be recorded: {e}"),
    })
}

fn ends_the_turn(mut result: CallToolResult) -> CallToolResult {
    let mut meta = result.meta.take().map(|m| m.0).unwrap_or_default();
    meta.insert(END_TURN_META_KEY.to_string(), serde_json::Value::Bool(true));
    result.meta = Some(Meta(meta));
    result
}

pub struct LoopReportClient {
    info: InitializeResult,
    context: PlatformExtensionContext,
}

impl LoopReportClient {
    pub fn new(context: PlatformExtensionContext) -> Result<Self> {
        let info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(EXTENSION_NAME.to_string(), "1.0.0".to_string())
                    .with_title("Loop"),
            )
            .with_instructions(
                indoc! {r#"
                This chat runs a loop: a tick is a turn whose prompt starts with "Loop tick".
                End every tick by calling `loop_report`, after rewriting the loop's state file;
                calling it ends the tick. Outside a tick the tool is refused.
            "#}
                .to_string(),
            );
        Ok(Self { info, context })
    }

    pub fn tools() -> Vec<Tool> {
        vec![Tool::new(
            LOOP_REPORT_TOOL_NAME.to_string(),
            TOOL_DESCRIPTION.to_string(),
            report_schema(),
        )
        .annotate(ToolAnnotations::from_raw(
            Some("Loop report".to_string()),
            Some(true),
            Some(false),
            Some(false),
            Some(false),
        ))]
    }

    async fn loop_report(
        &self,
        session_id: &str,
        arguments: Option<JsonObject>,
    ) -> Result<CallToolResult, String> {
        record_report(&self.context.session_manager, session_id, arguments).await?;
        Ok(ends_the_turn(CallToolResult::success(vec![Content::text(
            RECORDED,
        )])))
    }
}

#[async_trait]
impl McpClientTrait for LoopReportClient {
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
            LOOP_REPORT_TOOL_NAME => self.loop_report(&ctx.session_id, arguments).await,
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
