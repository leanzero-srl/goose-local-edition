//! The Rust suite over `loops.fixture.json` (the desktop's `components/loops/model.test.ts` runs
//! the very same file) and the parts only goosed has: the store, the seam's named refusal, the
//! templates, `wrote` and the prompt.

use chrono::{DateTime, Duration, Utc};
use goose_sdk_types::custom_requests::{
    LoopCadence, LoopControlAction, LoopEdit, LoopRecord, LoopRefusal, LoopRefusalCode,
    LoopRefuseReason, LoopStatus, LoopStatusReason, LoopTemplateId, LoopTickRecord,
    LoopsControlRequest, LoopsGetRequest, LoopsReadyRequest, LoopsStartRequest,
    LoopsTickRefusedRequest, LoopsUpdateRequest,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::prompt::{tick_prompt, PromptFacts};
use super::record::{self, parse_time, RawLoopValue};
use super::rules::*;
use super::{acp, seam, templates};

fn fixture() -> Value {
    serde_json::from_str(include_str!("loops.fixture.json")).expect("loops.fixture.json parses")
}

fn cases(section: &str) -> Vec<Value> {
    let cases = fixture()[section]
        .as_array()
        .unwrap_or_else(|| panic!("fixture section {section}"))
        .clone();
    assert!(!cases.is_empty(), "fixture section {section} is empty");
    cases
}

fn typed<T: DeserializeOwned>(value: &Value, what: &str) -> T {
    serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{what}: {e}\n{value}"))
}

fn name(case: &Value) -> String {
    case["name"].as_str().unwrap_or("?").to_string()
}

fn time(case: &Value, key: &str) -> DateTime<Utc> {
    parse_time(case[key].as_str().expect("a time")).expect("an RFC 3339 time")
}

#[test]
fn every_fixture_record_round_trips_through_serde_unchanged() {
    for case in cases("records") {
        let record: LoopRecord = typed(&case["record"], &name(&case));
        assert_eq!(
            serde_json::to_value(&record).unwrap(),
            case["record"],
            "{}",
            name(&case)
        );
    }
}

#[test]
fn the_cadence_grammar_and_its_labels_follow_the_fixture() {
    for case in cases("cadences") {
        let text = case["text"].as_str().unwrap();
        let seconds = crate::loop_clock::parse_cadence(text).map(|d| d.num_seconds());
        assert_eq!(seconds, case["seconds"].as_i64(), "{text:?}");
        let label = cadence_label(&LoopCadence::Every {
            every: text.to_string(),
        });
        assert_eq!(label, case["label"].as_str().unwrap(), "{text:?}");
    }
    for case in cases("cadenceLabels") {
        let cadence: LoopCadence = typed(&case["cadence"], "cadence");
        assert_eq!(cadence_label(&cadence), case["label"].as_str().unwrap());
    }
    for case in cases("durationWords") {
        assert_eq!(
            duration_words(case["seconds"].as_i64().unwrap()),
            case["words"].as_str().unwrap()
        );
    }
}

#[test]
fn next_tick_follows_the_fixture() {
    for case in cases("nextTick") {
        let record: LoopRecord = typed(&case["record"], &name(&case));
        let got = next_tick(
            &record,
            time(&case, "now"),
            case["reviewersPending"].as_bool().unwrap(),
        )
        .unwrap_or_else(|e| panic!("{}: {e}", name(&case)));
        let want: NextTickDecision = typed(&case["expect"], &name(&case));
        assert_eq!(got, want, "{}", name(&case));
    }
    for case in cases("nextTickErrors") {
        let record: LoopRecord = typed(&case["record"], &name(&case));
        assert!(
            next_tick(&record, time(&case, "now"), false).is_err(),
            "{} must be a named error",
            name(&case)
        );
    }
}

#[test]
fn every_decide_row_of_the_design_follows_the_fixture() {
    let decide_cases = cases("decide");
    assert!(decide_cases.len() >= 30);
    for case in decide_cases {
        let record: LoopRecord = typed(&case["record"], &name(&case));
        let facts: TickFacts = typed(&case["facts"], &name(&case));
        let got =
            decide_after_tick(&record, &facts).unwrap_or_else(|e| panic!("{}: {e}", name(&case)));
        let want: Decision = typed(&case["expect"], &name(&case));
        assert_eq!(got, want, "{}", name(&case));
    }
    for case in cases("decideErrors") {
        let record: LoopRecord = typed(&case["record"], &name(&case));
        let facts: TickFacts = typed(&case["facts"], &name(&case));
        assert!(
            decide_after_tick(&record, &facts).is_err(),
            "{} must be a named error",
            name(&case)
        );
    }
}

#[test]
fn stalled_follows_the_fixture() {
    for case in cases("stalled") {
        let prev: LoopTickRecord = typed(&case["prev"], "prev");
        let cur: LoopTickRecord = typed(&case["cur"], "cur");
        assert_eq!(
            stalled(&prev, &cur),
            case["expect"].as_bool().unwrap(),
            "{}",
            name(&case)
        );
    }
}

#[test]
fn the_effective_status_follows_the_fixture() {
    for case in cases("effective") {
        let record: LoopRecord = typed(&case["record"], &name(&case));
        let proof: Option<OwnerProof> = typed(&case["proof"], "proof");
        let (status, reason) = effective_status(&record, proof.as_ref());
        let want_status: LoopStatus = typed(&case["expect"]["status"], "status");
        let want_reason: Option<LoopStatusReason> = typed(&case["expect"]["reason"], "reason");
        assert_eq!(
            (status, reason),
            (want_status, want_reason),
            "{}",
            name(&case)
        );
    }
}

#[test]
fn every_status_sentence_follows_the_fixture() {
    for case in cases("sentences") {
        let record: LoopRecord = typed(&case["record"], &name(&case));
        let status: LoopStatus = typed(&case["status"], "status");
        let reason: Option<LoopStatusReason> = typed(&case["reason"], "reason");
        let got = status_sentence(
            &record,
            status,
            reason.as_ref(),
            time(&case, "now"),
            case["utcOffsetMinutes"].as_i64().unwrap() as i32,
        )
        .unwrap_or_else(|e| panic!("{}: {e}", name(&case)));
        let want: Sentence = typed(&case["expect"], &name(&case));
        assert_eq!(got, want, "{}", name(&case));
    }
    for case in cases("sentenceErrors") {
        let record: LoopRecord = typed(&case["record"], &name(&case));
        let status: LoopStatus = typed(&case["status"], "status");
        let reason: Option<LoopStatusReason> = typed(&case["reason"], "reason");
        assert!(
            status_sentence(&record, status, reason.as_ref(), time(&case, "now"), 0).is_err(),
            "{} must be a named error",
            name(&case)
        );
    }
}

#[test]
fn tick_ids_follow_the_fixture() {
    for case in cases("tickIds") {
        let id = case["id"].as_str().unwrap();
        let want: Option<TickId> = typed(&case["expect"], id);
        assert_eq!(parse_tick_id(id), want, "{id:?}");
    }
    for case in cases("tickIdMint") {
        let id = tick_id(
            case["loopId"].as_str().unwrap(),
            case["n"].as_u64().unwrap() as u32,
            case["uuid"].as_str().unwrap(),
        );
        assert_eq!(id, case["id"].as_str().unwrap());
        assert_eq!(
            parse_tick_id(&id).map(|t| t.n),
            case["n"].as_u64().map(|n| n as u32)
        );
    }
}

#[test]
fn tick_ranges_follow_the_fixture() {
    for case in cases("tickRanges") {
        let ids: Vec<Option<String>> = typed(&case["messageIds"], "ids");
        let ids: Vec<Option<&str>> = ids.iter().map(|i| i.as_deref()).collect();
        let ticks: Vec<LoopTickRecord> = typed(&case["ticks"], "ticks");
        let want: Vec<TickRange> = typed(&case["expect"], &name(&case));
        assert_eq!(tick_ranges(&ids, &ticks), want, "{}", name(&case));
    }
}

#[test]
fn steps_slots_and_the_state_file_follow_the_fixture() {
    for case in cases("renderSteps") {
        let facts: StepFacts = typed(&case["facts"], "facts");
        let want: RenderedSteps = typed(&case["expect"], &name(&case));
        assert_eq!(
            render_steps(case["steps"].as_str().unwrap(), &facts),
            want,
            "{}",
            name(&case)
        );
    }
    for case in cases("stepSlots") {
        let want: Vec<String> = typed(&case["slots"], "slots");
        assert_eq!(step_slots(case["steps"].as_str().unwrap()), want);
    }
    for case in cases("defaultStateFile") {
        assert_eq!(
            default_state_file(case["goal"].as_str().unwrap()),
            case["path"].as_str().unwrap()
        );
    }
}

#[test]
fn start_and_edit_validation_follows_the_fixture() {
    for case in cases("validate") {
        let edit: LoopEdit = typed(&case["edit"], &name(&case));
        let chat: ChatFacts = typed(&case["chat"], "chat");
        let got = validate_loop(&edit, &chat);
        match (
            &got,
            case["expect"].get("ok"),
            case["expect"].get("refusal"),
        ) {
            (Ok(valid), Some(ok), None) => {
                assert_eq!(serde_json::to_value(valid).unwrap(), *ok, "{}", name(&case))
            }
            (Err(refusal), None, Some(want)) => {
                let want: LoopRefusal = typed(want, "refusal");
                assert_eq!(*refusal, want, "{}", name(&case));
            }
            _ => panic!("{}: got {got:?}, want {}", name(&case), case["expect"]),
        }
    }
}

#[test]
fn the_loop_command_grammar_follows_the_fixture() {
    for case in cases("loopCommands") {
        let line = case["line"].as_str().unwrap();
        let want: Option<LoopCommand> = typed(&case["expect"], line);
        assert_eq!(parse_loop_command(line), want, "{line:?}");
    }
    assert!(parse_loop_command("/loop stop").unwrap().is_control());
    assert!(!parse_loop_command("/loop fix the tests")
        .unwrap()
        .is_control());
}

#[test]
fn the_output_tail_follows_the_fixture() {
    let words = |s: &str| s.split_whitespace().count();
    for case in cases("outputTail") {
        let got = output_tail(
            case["output"].as_str().unwrap(),
            case["window"].as_u64().unwrap() as usize,
            words,
        );
        assert_eq!(got, case["expect"].as_str().unwrap(), "{}", name(&case));
    }
}

#[test]
fn every_prompt_follows_the_fixture() {
    for case in cases("prompts") {
        let record: LoopRecord = typed(&case["record"], &name(&case));
        let facts: PromptFacts = typed(&case["facts"], "facts");
        let got = tick_prompt(&record, case["n"].as_u64().unwrap() as u32, &facts)
            .unwrap_or_else(|e| panic!("{}: {e}", name(&case)));
        assert_eq!(got, case["expect"].as_str().unwrap(), "{}", name(&case));
    }
}

// ---------------------------------------------------------------------------------------------
// Beyond the fixture
// ---------------------------------------------------------------------------------------------

#[test]
fn the_check_tail_is_one_sixty_fourth_of_the_window() {
    assert_eq!(check_tail_budget(262_144), 4_096);
    assert_eq!(check_tail_budget(32_768), 512);
    assert_eq!(check_tail_budget(63), 0);
}

#[tokio::test]
async fn the_output_tail_holds_with_goose_s_own_token_counter() {
    let counter = crate::token_counter::create_token_counter()
        .await
        .expect("goose's token counter");
    let output: String = (0..20_000)
        .map(|i| format!("test {i} ... ok\n"))
        .chain(std::iter::once(
            "FAILED: missing svc- accounts\n".to_string(),
        ))
        .collect();
    let window = 32_768;
    let tail = output_tail(&output, window, |s| counter.count_tokens(s));
    let budget = check_tail_budget(window);
    assert!(counter.count_tokens(tail) <= budget);
    assert!(tail.ends_with("FAILED: missing svc- accounts\n"));
    let start = output.len() - tail.len();
    let longer = &output[output[..start].char_indices().last().unwrap().0..];
    assert!(
        counter.count_tokens(longer) > budget,
        "one more character would fit, so the tail is not the longest"
    );
}

#[test]
fn every_template_uses_only_known_slots_and_passes_validation() {
    let all = templates::all();
    assert_eq!(
        all.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![
            LoopTemplateId::Quality,
            LoopTemplateId::UntilCheck,
            LoopTemplateId::Watch,
            LoopTemplateId::Blank
        ]
    );
    let chat = ChatFacts {
        working_dir: "/Users/mihai/work".into(),
        swarm_build: false,
    };
    for template in all {
        for slot in &template.slots {
            assert!(templates::SLOTS.contains(&slot.as_str()), "{slot}");
        }
        let edit = LoopEdit {
            goal: "Make every test pass".into(),
            template: template.id,
            steps: template.steps.clone(),
            cadence: template.suggested_cadence.clone(),
            state_file: default_state_file("Make every test pass"),
            check: template.needs_check.then(|| "pnpm test".to_string()),
            stop_after_ticks: None,
        };
        assert!(validate_loop(&edit, &chat).is_ok(), "{:?}", template.id);
    }
    assert!(templates::template(LoopTemplateId::UntilCheck).needs_check);
    assert!(templates::template(LoopTemplateId::Blank).steps.is_empty());
}

fn write_call(id: &str, tool: &str, path: &str) -> Vec<crate::conversation::message::Message> {
    use crate::conversation::message::Message;
    use rmcp::model::{CallToolRequestParams, CallToolResult, Content, Meta};
    let mut result = CallToolResult::success(vec![Content::text("ok")]);
    let mut meta = Meta::new();
    meta.0.insert(
        crate::agents::platform_extensions::developer::file_diff::FILE_DIFF_META_KEY.to_string(),
        json!({"path": path, "before": "file", "added": 1, "removed": 0, "unified": "@@"}),
    );
    result.meta = Some(meta);
    let call = CallToolRequestParams::new(tool.to_string());
    vec![
        Message::assistant().with_tool_request(id, Ok(call)),
        Message::user().with_tool_response(id, Ok(result)),
    ]
}

#[test]
fn wrote_lists_write_and_edit_diffs_without_the_state_file() {
    let wd = "/Users/mihai/work";
    let state = ".goose/loops/x/NOW.md";
    let mut messages = Vec::new();
    messages.extend(write_call("t1", "write", "scripts/gen.js"));
    messages.extend(write_call(
        "t2",
        "edit",
        "/Users/mihai/work/.goose/loops/x/NOW.md",
    ));
    messages.extend(write_call("t3", "edit", "scripts/gen.js"));
    messages.extend(write_call("t4", "shell", "scripts/other.js"));
    messages.extend(write_call("t5", "edit", "./.goose/loops/x/NOW.md"));
    messages.extend(write_call("t6", "write", "/tmp/elsewhere.txt"));
    assert_eq!(
        wrote(&messages, state, wd),
        vec![
            "scripts/gen.js".to_string(),
            "/tmp/elsewhere.txt".to_string()
        ]
    );
    assert!(wrote(&messages[..0], state, wd).is_empty());
}

// ---------------------------------------------------------------------------------------------
// The store and the handlers, over a real session store
// ---------------------------------------------------------------------------------------------

async fn store() -> (tempfile::TempDir, crate::session::SessionManager, String) {
    let dir = tempfile::tempdir().unwrap();
    let manager = crate::session::SessionManager::new(dir.path().to_path_buf());
    let session = manager
        .create_session(
            "/Users/mihai/work".into(),
            "loop chat".into(),
            crate::session::SessionType::User,
            crate::config::GooseMode::default(),
        )
        .await
        .unwrap();
    (dir, manager, session.id)
}

fn a_record(status: LoopStatus) -> LoopRecord {
    let now = parse_time("2026-09-27T22:00:00Z").unwrap();
    let mut record = record::new_record(
        "lp_0a1b2c3d".into(),
        LoopEdit {
            goal: "Make every test pass".into(),
            template: LoopTemplateId::Blank,
            steps: String::new(),
            cadence: LoopCadence::Every {
                every: "10m".into(),
            },
            state_file: ".goose/loops/x/NOW.md".into(),
            check: None,
            stop_after_ticks: None,
        },
        now,
    );
    record.status = status;
    record.next_tick = Some(goose_sdk_types::custom_requests::LoopNextTick {
        at: record::fmt_time(now + Duration::minutes(10)),
        reason: goose_sdk_types::custom_requests::LoopNextReason::First,
    });
    record
}

#[tokio::test]
async fn a_chat_without_a_loop_reads_none_and_nothing_is_written() {
    let (_dir, manager, id) = store().await;
    let got = acp::get(
        &manager,
        LoopsGetRequest {
            session_id: id.clone(),
        },
    )
    .await
    .unwrap();
    assert!(got.record.is_none() && got.error.is_none() && got.effective_status.is_none());
    let session = manager.get_session(&id, false).await.unwrap();
    assert!(session
        .extension_data
        .get_extension_state(record::EXTENSION_NAME, record::VERSION)
        .is_none());
    assert!(acp::list(&manager).await.unwrap().loops.is_empty());
}

#[tokio::test]
async fn an_unreadable_record_is_named_never_read_as_no_loop() {
    let (_dir, manager, id) = store().await;
    manager
        .update_extension_state::<RawLoopValue, _>(&id, |_| {
            Ok((RawLoopValue(json!({"id": 3})), ()))
        })
        .await
        .unwrap();
    let got = acp::get(
        &manager,
        LoopsGetRequest {
            session_id: id.clone(),
        },
    )
    .await
    .unwrap();
    assert!(got.record.is_none());
    assert!(got
        .error
        .as_deref()
        .is_some_and(|e| e.starts_with("The loop record could not be read")));
    let listed = acp::list(&manager).await.unwrap().loops;
    assert_eq!(listed.len(), 1);
    assert!(listed[0].status.is_none() && listed[0].error.is_some());
}

#[tokio::test]
async fn reads_derive_the_status_and_never_write() {
    let (_dir, manager, id) = store().await;
    let written = a_record(LoopStatus::Waiting);
    let stored = written.clone();
    record::update(&manager, &id, |_| Ok((stored, ())))
        .await
        .unwrap();
    let before = manager
        .get_session(&id, false)
        .await
        .unwrap()
        .extension_data;

    let got = acp::get(
        &manager,
        LoopsGetRequest {
            session_id: id.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(got.record.as_ref(), Some(&written));
    // No owner recorded: nothing runs its clock, so it reads as closed — derived, not written.
    assert_eq!(got.effective_status, Some(LoopStatus::Paused));
    assert_eq!(
        got.effective_reason,
        Some(LoopStatusReason::Closed { closed_at: None })
    );
    let listed = acp::list(&manager).await.unwrap().loops;
    assert_eq!(listed[0].status, Some(LoopStatus::Paused));
    assert_eq!(listed[0].next_tick_at, None);
    let after = manager
        .get_session(&id, false)
        .await
        .unwrap()
        .extension_data;
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(&after).unwrap()
    );

    record::update(&manager, &id, |r| {
        let mut r = r.expect("the record");
        r.status = LoopStatus::Ended;
        r.status_reason = Some(LoopStatusReason::ReachedCount { k: 3 });
        Ok((r, ()))
    })
    .await
    .unwrap();
    assert!(acp::looping(&manager).await.unwrap().is_empty());
    assert_eq!(acp::list(&manager).await.unwrap().loops.len(), 1);
}

fn is_runner_absent(refusal: &Option<LoopRefusal>) -> bool {
    refusal
        .as_ref()
        .is_some_and(|r| r.code == LoopRefusalCode::RunnerAbsent && r.reason == seam::RUNNER_ABSENT)
}

#[tokio::test]
async fn with_no_runner_every_mutation_answers_the_named_refusal_and_writes_nothing() {
    assert!(!seam::runner_installed());
    let (_dir, manager, id) = store().await;
    let start = acp::start(
        &manager,
        LoopsStartRequest {
            session_id: id.clone(),
            goal: "Make every test pass".into(),
            template: LoopTemplateId::Blank,
            steps: String::new(),
            cadence: LoopCadence::SelfPaced,
            state_file: ".goose/loops/x/NOW.md".into(),
            check: None,
            stop_after_ticks: None,
        },
    )
    .await
    .unwrap();
    assert!(start.record.is_none() && is_runner_absent(&start.refusal));

    let update = acp::update(
        &manager,
        LoopsUpdateRequest {
            session_id: id.clone(),
            patch: LoopEdit::default(),
        },
    )
    .await
    .unwrap();
    assert!(is_runner_absent(&update.refusal));
    let control = acp::control(LoopsControlRequest {
        session_id: id.clone(),
        action: LoopControlAction::TickNow,
    })
    .await
    .unwrap();
    assert!(is_runner_absent(&control.refusal));
    let refused = acp::tick_refused(LoopsTickRefusedRequest {
        session_id: id.clone(),
        loop_id: "lp_0a1b2c3d".into(),
        n: 1,
        reason: LoopRefuseReason::QueuedMessage,
    })
    .await
    .unwrap();
    assert!(is_runner_absent(&refused.refusal));
    let ready = acp::ready(LoopsReadyRequest {
        session_id: id.clone(),
    })
    .await
    .unwrap();
    assert!(!ready.reoffered && is_runner_absent(&ready.refusal));
    let wake = acp::wake().await.unwrap();
    assert!(wake.rearmed == 0 && is_runner_absent(&wake.refusal));

    assert_eq!(
        seam::prove_owner(&goose_sdk_types::custom_requests::LoopOwner {
            goosed_pid: 1,
            goosed_started_at: 1,
            app_pid: 1
        }),
        OwnerProof::Unproven {
            why: seam::RUNNER_ABSENT.to_string()
        }
    );
    let session = manager.get_session(&id, false).await.unwrap();
    assert!(session
        .extension_data
        .get_extension_state(record::EXTENSION_NAME, record::VERSION)
        .is_none());
}

#[test]
fn the_templates_method_serves_all_four() {
    let served = acp::templates().templates;
    assert_eq!(served.len(), 4);
    assert_eq!(served[0].name, "Software quality loop");
    assert_eq!(
        served[1].suggested_cadence,
        LoopCadence::BackToBack,
        "until a check passes runs back to back"
    );
}
