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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use futures::StreamExt;
    use goose_providers::conversation::token_usage::{ProviderUsage, Usage};
    use goose_providers::errors::ProviderError;
    use goose_providers::model::ModelConfig;
    use goose_sdk_types::custom_requests::{LoopCadence, LoopRecord};
    use rmcp::model::{CallToolRequestParams, RawContent};
    use rmcp::object;
    use serde_json::Value;

    use super::*;
    use crate::agents::SessionConfig;
    use crate::conversation::message::Message;
    use crate::providers::base::{stream_from_single_message, MessageStream, Provider};
    use crate::session_loops::agent_sync::{sync_on, testing::*};

    fn every_10m() -> LoopCadence {
        LoopCadence::Every {
            every: "10m".into(),
        }
    }

    fn args(value: Value) -> Option<JsonObject> {
        match value {
            Value::Object(object) => Some(object),
            _ => panic!("arguments are an object"),
        }
    }

    fn client(manager: &Arc<SessionManager>) -> LoopReportClient {
        LoopReportClient::new(PlatformExtensionContext {
            extension_manager: None,
            session_manager: Arc::clone(manager),
            session: None,
            use_login_shell_path: false,
        })
        .unwrap()
    }

    async fn call(
        manager: &Arc<SessionManager>,
        session_id: &str,
        arguments: Option<JsonObject>,
    ) -> CallToolResult {
        client(manager)
            .call_tool(
                &ToolCallContext::new(session_id.to_string(), None, None),
                LOOP_REPORT_TOOL_NAME,
                arguments,
                CancellationToken::new(),
            )
            .await
            .unwrap()
    }

    fn text(result: &CallToolResult) -> String {
        result
            .content
            .iter()
            .filter_map(|c| match &c.raw {
                RawContent::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect()
    }

    fn ends_turn(result: &CallToolResult) -> bool {
        result
            .meta
            .as_ref()
            .and_then(|m| m.0.get(END_TURN_META_KEY))
            == Some(&Value::Bool(true))
    }

    async fn in_flight(cadence: LoopCadence) -> (tempfile::TempDir, Arc<SessionManager>, String) {
        let (dir, manager, id) = a_chat().await;
        store(&manager, &id, a_loop(cadence, 1, true)).await;
        (dir, manager, id)
    }

    fn last_report(record: &LoopRecord) -> Option<LoopReport> {
        record.ticks.last().and_then(|t| t.report.clone())
    }

    /// The local engines' qwen3_coder parser keeps a parameter's value as text only when its
    /// schema type is a plain string; any other type is `ast.literal_eval`ed and a word refuses.
    #[test]
    fn the_schema_is_flat_and_every_argument_a_plain_string() {
        let schema = report_schema();
        assert_eq!(schema["type"], "object");
        let properties = schema["properties"].as_object().unwrap();
        let names: Vec<&str> = properties.keys().map(String::as_str).collect();
        for name in [
            "verdict",
            "summary",
            "next_step",
            "next_in",
            "next_reason",
            "blocked_on",
        ] {
            assert!(names.contains(&name), "{name} is in the schema: {names:?}");
        }
        assert_eq!(properties.len(), 6);
        for (name, property) in properties {
            assert_eq!(
                property["type"],
                Value::String("string".into()),
                "{name} is a plain string"
            );
            assert!(property.get("properties").is_none() && property.get("items").is_none());
        }
        assert_eq!(
            properties["verdict"]["enum"],
            serde_json::json!(["progress", "done", "blocked"])
        );
        assert_eq!(
            schema["required"],
            serde_json::json!(["verdict", "summary", "next_step"])
        );
        let tool = &LoopReportClient::tools()[0];
        assert_eq!(tool.name, LOOP_REPORT_TOOL_NAME);
        assert_eq!(
            tool.annotations.as_ref().and_then(|a| a.read_only_hint),
            Some(true),
            "read-only, so an approve mode never asks the person to approve a tick's report"
        );
    }

    #[tokio::test]
    async fn each_verdict_is_recorded_on_the_tick_in_flight_and_ends_the_turn() {
        for (verdict, blocked_on) in [
            ("progress", None),
            ("done", None),
            (" Blocked ", Some("which CSV delimiter the owner wants")),
        ] {
            let (_dir, manager, id) = in_flight(every_10m()).await;
            let mut call_args = serde_json::json!({
                "verdict": verdict,
                "summary": "  Added svc- accounts; node scripts/validate_users.js exited 0  ",
                "next_step": "cover case-only duplicate emails in generate_users.js",
            });
            if let Some(b) = blocked_on {
                call_args["blocked_on"] = b.into();
            }
            let result = call(&manager, &id, args(call_args)).await;
            assert_eq!(text(&result), RECORDED, "{verdict}");
            assert_ne!(result.is_error, Some(true));
            assert!(
                ends_turn(&result),
                "{verdict}: a valid report ends the turn"
            );

            let report = last_report(&stored(&manager, &id).await.unwrap()).unwrap();
            assert_eq!(
                report.verdict,
                match verdict.trim().to_lowercase().as_str() {
                    "progress" => LoopVerdict::Progress,
                    "done" => LoopVerdict::Done,
                    _ => LoopVerdict::Blocked,
                }
            );
            assert_eq!(
                report.summary,
                "Added svc- accounts; node scripts/validate_users.js exited 0"
            );
            assert_eq!(
                report.next_step,
                "cover case-only duplicate emails in generate_users.js"
            );
            assert_eq!(report.blocked_on.as_deref(), blocked_on);
            let record = stored(&manager, &id).await.unwrap();
            assert!(
                record.ticks[0].report.is_none(),
                "only the tick in flight is written"
            );
        }
    }

    #[tokio::test]
    async fn a_refused_report_writes_nothing_and_does_not_end_the_turn() {
        let cases = [
            (
                serde_json::json!({"summary": "s", "next_step": "n"}),
                LoopCadence::SelfPaced,
                "verdict is missing",
            ),
            (
                serde_json::json!({"verdict": "maybe", "summary": "s", "next_step": "n"}),
                LoopCadence::SelfPaced,
                "verdict \"maybe\" is not progress, done or blocked.",
            ),
            (
                serde_json::json!({"verdict": "progress", "summary": "   ", "next_step": "n"}),
                LoopCadence::SelfPaced,
                "summary is empty",
            ),
            (
                serde_json::json!({"verdict": "progress", "summary": "s"}),
                LoopCadence::SelfPaced,
                "next_step is empty",
            ),
            (
                serde_json::json!({"verdict": "blocked", "summary": "s", "next_step": "n"}),
                LoopCadence::SelfPaced,
                "verdict is blocked but blocked_on is empty",
            ),
            (
                serde_json::json!({"verdict": "progress", "summary": "s", "next_step": "n", "next_in": "soon"}),
                LoopCadence::SelfPaced,
                "next_in \"soon\" is not a number and s, m or h",
            ),
            (
                serde_json::json!({"verdict": "progress", "summary": 42, "next_step": "n"}),
                LoopCadence::SelfPaced,
                "The arguments could not be read",
            ),
        ];
        for (call_args, cadence, words) in cases {
            let (_dir, manager, id) = in_flight(cadence).await;
            let before = stored(&manager, &id).await;
            let result = call(&manager, &id, args(call_args.clone())).await;
            assert_eq!(result.is_error, Some(true), "{call_args}");
            assert!(
                text(&result).contains(words),
                "{call_args}: {}",
                text(&result)
            );
            assert!(
                !ends_turn(&result),
                "{call_args}: a refusal lets the model call again"
            );
            assert_eq!(stored(&manager, &id).await, before, "{call_args}");
        }

        let (_dir, manager, id) = in_flight(LoopCadence::SelfPaced).await;
        let result = call(&manager, &id, None).await;
        assert!(text(&result).contains("needs its arguments") && !ends_turn(&result));
    }

    #[tokio::test]
    async fn outside_a_tick_the_report_is_refused_by_name() {
        let (_dir, manager, no_loop) = a_chat().await;
        let good =
            || args(serde_json::json!({"verdict": "progress", "summary": "s", "next_step": "n"}));
        let result = call(&manager, &no_loop, good()).await;
        assert_eq!(text(&result), format!("Error: {NO_TICK}"));
        assert!(!ends_turn(&result));
        assert!(
            stored(&manager, &no_loop).await.is_none(),
            "nothing written"
        );

        for between_ticks in [a_loop(every_10m(), 2, false), a_loop(every_10m(), 0, false)] {
            let (_dir, manager, id) = a_chat().await;
            store(&manager, &id, between_ticks.clone()).await;
            let result = call(&manager, &id, good()).await;
            assert_eq!(text(&result), format!("Error: {NO_TICK}"));
            assert!(!ends_turn(&result));
            assert_eq!(stored(&manager, &id).await, Some(between_ticks));
        }
    }

    #[tokio::test]
    async fn the_delay_is_checked_only_where_goose_decides_when() {
        let (_dir, manager, id) = in_flight(LoopCadence::SelfPaced).await;
        let result = call(
            &manager,
            &id,
            args(serde_json::json!({
                "verdict": "progress", "summary": "s", "next_step": "n",
                "next_in": "90m", "next_reason": "the nightly build lands at 01:00"
            })),
        )
        .await;
        assert!(ends_turn(&result));
        let report = last_report(&stored(&manager, &id).await.unwrap()).unwrap();
        assert_eq!(report.next_in.as_deref(), Some("90m"));
        assert_eq!(
            report.next_reason.as_deref(),
            Some("the nightly build lands at 01:00")
        );

        // No delay named: recorded as absent, and the loop then waits for the person by name
        // (rules::next_tick → waiting_you); a JSON null (the model's literal `null`) is the same.
        let (_dir, manager, id) = in_flight(LoopCadence::SelfPaced).await;
        let result = call(
            &manager,
            &id,
            args(serde_json::json!({
                "verdict": "progress", "summary": "s", "next_step": "n", "next_in": null
            })),
        )
        .await;
        assert!(ends_turn(&result));
        let report = last_report(&stored(&manager, &id).await.unwrap()).unwrap();
        assert!(report.next_in.is_none() && report.next_reason.is_none());

        // A fixed cadence never reads next_in; it is kept as the model wrote it.
        let (_dir, manager, id) = in_flight(every_10m()).await;
        let result = call(
            &manager,
            &id,
            args(serde_json::json!({
                "verdict": "progress", "summary": "s", "next_step": "n", "next_in": "soon"
            })),
        )
        .await;
        assert!(ends_turn(&result));
        let report = last_report(&stored(&manager, &id).await.unwrap()).unwrap();
        assert_eq!(report.next_in.as_deref(), Some("soon"));
    }

    #[tokio::test]
    async fn a_second_report_in_the_same_tick_replaces_the_first() {
        let (_dir, manager, id) = in_flight(every_10m()).await;
        for step in ["first next step", "second next step"] {
            let result = call(
                &manager,
                &id,
                args(serde_json::json!({"verdict": "progress", "summary": "s", "next_step": step})),
            )
            .await;
            assert!(ends_turn(&result));
        }
        let record = stored(&manager, &id).await.unwrap();
        assert_eq!(last_report(&record).unwrap().next_step, "second next step");
        assert_eq!(record.ticks.len(), 2);
    }

    /// Each stream call answers with the next scripted assistant message.
    struct ScriptedProvider {
        script: Vec<Message>,
        calls: AtomicUsize,
        offered: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Provider for ScriptedProvider {
        fn get_name(&self) -> &str {
            "scripted"
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            *self.offered.lock().unwrap() = tools.iter().map(|t| t.name.to_string()).collect();
            let message = self.script.get(call).cloned().unwrap_or_else(|| {
                Message::assistant().with_text("the turn went on after the last scripted call")
            });
            Ok(stream_from_single_message(
                message,
                ProviderUsage::new("scripted".into(), Usage::new(Some(10), Some(5), Some(15))),
            ))
        }
    }

    fn report_call(id: &str, verdict: &str, next_step: &str) -> Message {
        Message::assistant().with_tool_request(
            id,
            Ok(
                CallToolRequestParams::new(LOOP_REPORT_TOOL_NAME).with_arguments(object!({
                    "verdict": verdict,
                    "summary": "ran the tests: 3 failed",
                    "next_step": next_step,
                })),
            ),
        )
    }

    async fn run_tick(script: Vec<Message>) -> (Arc<ScriptedProvider>, Option<LoopReport>) {
        let (_dir, manager, id) = in_flight(every_10m()).await;
        let agent = an_agent(&manager);
        sync_on(&agent, &id, &stored(&manager, &id).await.unwrap())
            .await
            .unwrap();
        let provider = Arc::new(ScriptedProvider {
            script,
            calls: AtomicUsize::new(0),
            offered: Mutex::new(Vec::new()),
        });
        agent
            .update_provider(provider.clone(), ModelConfig::new("scripted-model"), &id)
            .await
            .unwrap();
        let reply = agent
            .reply(
                Message::user().with_text("Loop tick 2 — \"Make every test pass\""),
                SessionConfig {
                    id: id.clone(),
                    schedule_id: None,
                    max_turns: None,
                    retry_config: None,
                },
                None,
            )
            .await
            .unwrap();
        tokio::pin!(reply);
        while let Some(event) = reply.next().await {
            event.unwrap();
        }
        let report = last_report(&stored(&manager, &id).await.unwrap());
        (provider, report)
    }

    #[tokio::test]
    async fn a_valid_report_ends_the_turn_in_the_agent_loop() {
        let (provider, report) = run_tick(vec![report_call(
            "call_report",
            "progress",
            "fix the parser",
        )])
        .await;
        assert!(
            provider
                .offered
                .lock()
                .unwrap()
                .iter()
                .any(|n| n == LOOP_REPORT_TOOL_NAME),
            "a loop chat's model is offered loop_report"
        );
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            1,
            "the turn ends at the report; the model is not called again"
        );
        assert_eq!(report.unwrap().next_step, "fix the parser");
    }

    #[tokio::test]
    async fn an_invalid_report_keeps_the_turn_open_so_the_model_can_retry() {
        let (provider, report) = run_tick(vec![
            report_call("call_bad", "maybe", "fix the parser"),
            report_call("call_good", "progress", "fix the parser properly"),
        ])
        .await;
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            2,
            "the refusal went back to the model, its retry ended the turn"
        );
        assert_eq!(report.unwrap().next_step, "fix the parser properly");
    }
}
