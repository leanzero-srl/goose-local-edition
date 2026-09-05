//! THE ASK HANDSHAKE: the clarify question type, the detached/interactive ask, and the one door
//! from the opener's qualified decisions to the payload. Sibling module under the
//! incremental-split law (development_gates::swarm_rs_line_count_only_decreases) — moved
//! verbatim from swarm.rs, paying for r6e E10's wiring; every item keeps its own WHY.

use std::path::Path;

use console::style;

use super::opener::OpenDecision;
use super::EventSink;

/// Schema for the GOOSE_SWARM_ASK clarifying-question generator: a flat list of interrogative strings.
/// One clarify/review question. `options` are 2-4 concrete CHOICES the user can pick with one click (they can
/// always type their own instead); empty when the question is genuinely open-ended.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct ClarifyQuestion {
    pub(super) question: String,
    #[serde(default)]
    pub(super) options: Vec<String>,
    /// Which open decision / uncertainty this question settles — surfaced in the panel as the question's
    /// rationale. Optional (empty omitted from the wire) so old consumers are unaffected.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(super) resolves: String,
}

/// Confidence-gated clarifying questions (GOOSE_SWARM_ASK_FLOOR). When the plan-confidence meter is below
/// the floor, the swarm asks the USER rather than guessing — local models are weak, so asking beats a
/// confident wrong decomposition. Interactive TTY -> cliclack prompts. Detached (no TTY: the autonomous
/// harness or an eval) -> write the questions to `.swarm/clarify-questions.json`, emit a `low_confidence_ask`
/// event, and BLOCK-poll for `.swarm/clarify-answers.json` (an attending harness MAY answer as the
/// human; the benchmark leaves it unanswered — r5's timeout event says "no answers arrived") up to
/// `wait_secs`, then proceed. Returns a Q&A block to fold into the planner findings, or "" if unanswered.
pub(super) async fn ask_clarifying_questions(
    questions: &[ClarifyQuestion],
    cwd: &Path,
    // plan_conf: the planner's confidence in its own breakdown, 0-100 — or None when NOTHING MEASURED IT,
    // which is not the same as zero. The arithmetic agreement metric was deleted with the multi-draft vote
    // (section 8), so nothing computes this any more and the call site passed a literal `0`. The panel
    // renders any number as a red `conf N` chip, so every run displayed "conf 0" — reading as "the planner
    // has no confidence in this plan at all". It was a placeholder sitting there looking like a verdict.
    plan_conf: Option<u8>,
    // The opener's OWN list of open decisions, so the ask can report itself honestly. The old
    // `breakdown: Option<serde_json::Value>` arg was DEAD — the only live call site passed None,
    // so open_decisions_total/not_asked read 0/0 forever and the GUESSED warning below was
    // unreachable while r5 asked 3 of 5 decisions and silently guessed 2.
    open_decisions: &[String],
    wait_secs: u64,
    proxy: Option<tokio::task::JoinHandle<()>>,
    sink: &dyn EventSink,
) -> String {
    use std::io::IsTerminal;
    if questions.is_empty() {
        return String::new();
    }
    let dir = cwd.join(".swarm");
    let _ = std::fs::create_dir_all(&dir);
    let qpath = dir.join("clarify-questions.json");
    let apath = dir.join("clarify-answers.json");
    let _ = std::fs::remove_file(&apath); // never read a stale answer from a previous gate
    let file_obj = serde_json::json!({
        "plan_confidence": plan_conf,  // null when unmeasured — never a fabricated 0
        "questions": questions,
        "answer_file": ".swarm/clarify-answers.json",
        "how_to_answer": "Write {\"answers\":[...one string per question, same order; pick an option or your own words...], \"guidance\":\"...free-form: anything else to change about the plan...\"} to answer_file (a bare JSON array of answers also works). The swarm is BLOCKED on it and will re-plan with your answers + guidance.",
    });
    if let Err(e) = std::fs::write(
        &qpath,
        serde_json::to_string_pretty(&file_obj).unwrap_or_default(),
    ) {
        eprintln!(
            "  warning: could not write clarify questions to {} ({e}) — the harness has nothing to answer",
            qpath.display()
        );
    }
    // NAME WHAT WAS DROPPED. Every open decision that is not asked is GUESSED — the exact outcome
    // no_tools_means_ask exists to prevent. MEASURED: r5's probe found 5 material open decisions,
    // all 5 on the spec's explicit "do NOT guess them" list, and 3 were asked (the since-killed
    // ask_max_q truncation). The other 2 were invented in silence, and the only way to notice was
    // to diff the opener's open_decisions against the questions by hand. The live planner path now
    // asks every open decision (one prompt costs the same for 5 as for 3), so not_asked is 0 there;
    // this reporting is the tripwire that names the drop, verbatim, if any caller truncates again.
    let open_n = open_decisions.len();
    let not_asked = open_n.saturating_sub(questions.len());
    let mut ask_evt = serde_json::json!({
        "event": "low_confidence_ask",
        "plan_confidence": plan_conf,  // null when unmeasured — never a fabricated 0
        "questions": questions,
        "open_decisions_total": open_n,
        "open_decisions_not_asked": not_asked,
    });
    if not_asked > 0 {
        // The decisions that will be guessed, VERBATIM — a count alone forces the by-hand diff
        // this reporting exists to end. Exact for the planner path, where each question's text
        // IS its decision verbatim; a caller that rephrases would list every unmatched text,
        // which is still the honest read ("these decision texts were never asked as written").
        let asked: std::collections::HashSet<&str> =
            questions.iter().map(|q| q.question.as_str()).collect();
        let guessed: Vec<&String> = open_decisions
            .iter()
            .filter(|d| !asked.contains(d.as_str()))
            .collect();
        ask_evt["open_decisions_not_asked_detail"] = serde_json::json!(guessed);
    }
    sink.write_value(ask_evt);
    if not_asked > 0 {
        eprintln!(
            "  {} asking {} of {} open decision(s) — the other {} will be GUESSED",
            style("!").yellow().bold(),
            questions.len(),
            open_n,
            not_asked
        );
    }

    // "Plan confidence 0/100" is a lie when nothing measured it. Say nothing rather than a number.
    let conf_label = match plan_conf {
        Some(c) => format!("Plan confidence {c}/100 — "),
        None => String::new(),
    };
    let mut answers: Vec<String> = Vec::new();
    let mut guidance = String::new();
    // INTERACTIVE only when BOTH stdin AND stdout are real terminals. A capture harness that pipes stdout
    // (or the autonomous loop / evals) is detached -> the file handshake, never a timeout-less cliclack
    // prompt that could hang forever on a PTY-backed child. GOOSE_SWARM_ASK_FILE=1 forces the file path.
    let force_file = std::env::var("GOOSE_SWARM_ASK_FILE")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "on" | "true" | "yes"))
        .unwrap_or(false);
    let interactive =
        !force_file && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if interactive {
        eprintln!(
            "{}",
            style(format!(
                "{conf_label}{} quick question(s) to get this right:",
                questions.len()
            ))
            .yellow()
            .bold()
        );
        for q in questions {
            let prompt = if q.options.is_empty() {
                q.question.clone()
            } else {
                format!("{} [{}]", q.question, q.options.join(" / "))
            };
            let a: String = cliclack::input(prompt)
                .default_input(q.options.first().map(String::as_str).unwrap_or(""))
                .interact()
                .unwrap_or_default();
            answers.push(a);
        }
    } else {
        eprintln!(
            "{}",
            style(format!(
                "{conf_label}wrote {} question(s) to {}; BLOCKING for answers in {} ({}) ...",
                questions.len(),
                qpath.display(),
                apath.display(),
                if proxy.is_some() {
                    "a node is answering these; the run waits for it, not for a clock".to_string()
                } else {
                    format!("up to {wait_secs}s, then goose decides these itself")
                }
            ))
            .yellow()
        );
        let mut waited = 0u64;
        let mut proxy_settled = false;
        loop {
            if let Ok(s) = std::fs::read_to_string(&apath) {
                let val = serde_json::from_str::<serde_json::Value>(&s).ok();
                if let Some(g) = val
                    .as_ref()
                    .and_then(|v| v.get("guidance"))
                    .and_then(|g| g.as_str())
                {
                    guidance = g.trim().to_string();
                }
                let parsed: Option<Vec<String>> =
                    serde_json::from_str::<Vec<String>>(&s).ok().or_else(|| {
                        val.as_ref().and_then(|val| {
                            val.get("answers").and_then(|a| a.as_array()).map(|arr| {
                                arr.iter()
                                    .map(|x| x.as_str().unwrap_or("").to_string())
                                    .collect()
                            })
                        })
                    });
                // Unblock as soon as the user submits per-question answers OR free-form guidance (they may
                // skip the questions entirely and just tell goose what to change).
                if parsed.is_some() || !guidance.is_empty() {
                    if let Some(a) = parsed {
                        answers = a;
                    }
                    eprintln!(
                        "{}",
                        style("clarifications received — continuing with the answers").green()
                    );
                    break;
                }
            }
            // NO DEADLINE OF ITS OWN WHILE A PROXY IS IN FLIGHT.
            //
            // MEASURED, run swarm-3node-r0: clarify_proxy_armed 06:49:09 -> low_confidence_ask_timeout
            // {waited_secs:5} 06:49:14 -> clarify_proxy_answered 06:49:56. The proxy's three answers — the
            // colour palette, HTTP 409 on a concurrent sync, ThreadingHTTPServer — landed 42s AFTER the only
            // reader had stopped reading, so OPEN's plan was built from guesses while `clarify_proxy_answered`
            // and the console both reported that a node had answered. TWO INDEPENDENT CLOCKS ON ONE
            // HANDSHAKE: `proxy_after` bounds the HUMAN, `wait_secs` bounded this reader, and a 27B answering
            // three product questions is slower than the shorter of the two. Whichever number is smaller
            // silently wins, which is how a wait on a MODEL got reintroduced after §8 deleted them all.
            //
            // So the exit condition is the proxy TASK, never a duration. Both its arms — Ok, and the Err that
            // writes conventional defaults — write the answer file before returning, so a finished handle
            // means the bytes are already on disk: settle once, re-read, parse. A proxy that panicked leaves
            // no file and is reported as itself rather than disguised as a timeout.
            if let Some(h) = &proxy {
                if h.is_finished() {
                    if proxy_settled {
                        sink.write_value(serde_json::json!({
                            "event": "clarify_proxy_unusable",
                            "questions_unanswered": questions.len(),
                            "detail": "the proxy task ended without leaving a readable answer file",
                        }));
                        return String::new();
                    }
                    proxy_settled = true;
                    continue;
                }
            }
            if proxy.is_none() && waited >= wait_secs {
                eprintln!(
                    "{}",
                    style(format!(
                        "no answers within {wait_secs}s — proceeding with the current plan. The fleet sat \
                         IDLE for that entire window and the questions goose asked are now decided by goose."
                    ))
                    .yellow()
                );
                // THE 30-MINUTE INVISIBLE HOLE.
                //
                // `low_confidence_answered` is emitted ONLY when an answer actually arrives (`if any` below).
                // This path — the wait expiring — returned SILENTLY. So an unanswered ask left NO event at
                // all: the whole window is invisible in the log except as an unexplained gap between
                // low_confidence_ask and whatever came next.
                //
                // MEASURED: ask_wait_secs defaults to 1800 = 30 MINUTES. loop-03 has exactly this — a
                // low_confidence_ask with no answered event and a 31.4-minute hole after it. And it fooled a
                // measurement TODAY: an analysis pass labelled that gap "human think-time" purely from the
                // preceding event's name, inflating measured human wait 5.2 -> 36.5 min (7x). It is neither
                // human time nor compute — it is the engine idling at a wall.
                //
                // A run that waited half an hour and then decided the user's product questions ITSELF must
                // say so. This is the same class as the clarity probe returning None: a silence that reads
                // exactly like a success.
                sink.write_value(serde_json::json!({
                    "event": "low_confidence_ask_timeout",
                    "waited_secs": wait_secs,
                    "questions_unanswered": questions.len(),
                    "detail": "no answers arrived; the fleet idled for the whole window and goose is now \
                               deciding these itself",
                }));
                return String::new();
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            waited += 5;
        }
    }

    let mut block = String::from(
        "\n\nUSER CLARIFICATIONS (authoritative — they resolve ambiguity in the spec above; honor them):\n",
    );
    let mut any = false;
    for (i, q) in questions.iter().enumerate() {
        let a = answers.get(i).map(|s| s.trim()).unwrap_or("");
        if !a.is_empty() {
            block.push_str(&format!("Q: {}\nA: {a}\n", q.question));
            any = true;
        }
    }
    if !guidance.trim().is_empty() {
        block.push_str(&format!(
            "The user also gave this free-form direction — treat it as a top-priority requirement: {}\n",
            guidance.trim()
        ));
        any = true;
    }
    if any {
        sink.write_value(serde_json::json!({ "event": "low_confidence_answered" }));
        block
    } else {
        String::new()
    }
}

/// The one door from the opener's QUALIFIED decisions to the ASK payload: the rendered line is the
/// question (the identity the user's answer is matched on), and the opener's options ride
/// STRUCTURED so the harness, `clarify-questions.json` and the desktop clarify card offer them as
/// one-click answers. r6d seq 91 (low_confidence_ask) carried `options: []` on all three
/// questions while every choice sat inside the line text — E5 qualified them, this hands them on.
pub(super) fn clarify_questions(decisions: &[OpenDecision]) -> Vec<ClarifyQuestion> {
    decisions
        .iter()
        .map(|d| ClarifyQuestion {
            question: d.line.clone(),
            options: d.options.clone(),
            resolves: String::new(),
        })
        .collect()
}

#[cfg(test)]
mod clarify_proxy_tests {
    use super::super::*;
    use super::*;

    fn q(text: &str) -> ClarifyQuestion {
        ClarifyQuestion {
            question: text.to_string(),
            options: vec![],
            resolves: String::new(),
        }
    }

    /// THE REGRESSION: the reader must wait for the PROXY, not for a clock of its own.
    ///
    /// Run swarm-3node-r0 measured `low_confidence_ask_timeout {waited_secs:5}` at 06:49:14 and
    /// `clarify_proxy_answered` at 06:49:56 — the answers landed 42s after the only reader stopped
    /// reading, so the plan was built from guesses while the log said a node had answered.
    ///
    /// The timings below reproduce that shape rather than describe it: `wait_secs` is 1 and the proxy
    /// takes 6s, so the reader's second poll is where the old code bailed. It returns empty on the
    /// fire-and-forget version and the answer only while the handle is the exit condition.
    #[tokio::test(start_paused = true)]
    async fn a_slow_proxy_is_waited_for_never_timed_out() {
        let dir = tempfile::tempdir().unwrap();
        let apath = dir.path().join(".swarm").join("clarify-answers.json");
        let proxy = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(6)).await;
            std::fs::create_dir_all(apath.parent().unwrap()).unwrap();
            std::fs::write(
                &apath,
                serde_json::json!({"answers": ["sqlite"], "source": "proxy"}).to_string(),
            )
            .unwrap();
        });
        let out = ask_clarifying_questions(
            &[q("storage: sqlite or a json file?")],
            dir.path(),
            None,
            &["storage: sqlite or a json file?".to_string()],
            1,
            Some(proxy),
            &NullSink,
        )
        .await;
        assert!(
            out.contains("sqlite"),
            "a proxy answer that arrives after wait_secs must still reach the plan; got {out:?}"
        );
    }

    /// A proxy that dies without writing reports itself, and does not hang the run either.
    #[tokio::test(start_paused = true)]
    async fn a_proxy_that_writes_nothing_unblocks_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let proxy = tokio::spawn(async {});
        let out = ask_clarifying_questions(
            &[q("storage: sqlite or a json file?")],
            dir.path(),
            None,
            &["storage: sqlite or a json file?".to_string()],
            1,
            Some(proxy),
            &NullSink,
        )
        .await;
        assert!(out.is_empty(), "got {out:?}");
    }

    /// With no proxy armed the human deadline is untouched — this fix removes a wait on a MODEL, not the
    /// bound on a HUMAN.
    #[tokio::test(start_paused = true)]
    async fn without_a_proxy_the_human_wait_still_expires() {
        let dir = tempfile::tempdir().unwrap();
        let out = ask_clarifying_questions(
            &[q("storage: sqlite or a json file?")],
            dir.path(),
            None,
            &["storage: sqlite or a json file?".to_string()],
            0,
            None,
            &NullSink,
        )
        .await;
        assert!(out.is_empty(), "got {out:?}");
    }

    /// ASK-TRUTH, through the PRIMARY computation. The emitter's old `breakdown` arg was passed
    /// None at the only live site, so open_decisions_total/not_asked were 0/0 forever and r5's
    /// "asked 3 of 5, guessed 2" was invisible — and every test here passed None too, so the
    /// counting code had ZERO coverage. This test feeds 5 real decisions and 3 questions and
    /// reads the event a run would write: total 5, not_asked 2, the guessed two named VERBATIM.
    #[tokio::test(start_paused = true)]
    async fn the_ask_event_counts_every_open_decision_and_names_the_guessed() {
        #[derive(Default)]
        struct ValueSink(Mutex<Vec<serde_json::Value>>);
        impl EventSink for ValueSink {
            fn emit(&self, _event: &SwarmEvent) {}
            fn write_value(&self, value: serde_json::Value) {
                self.0.lock().unwrap().push(value);
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let decisions: Vec<String> = [
            "storage: sqlite or a json file?",
            "auth: sessions or tokens?",
            "currency: integer cents or Decimal?",
            "ids: uuid or sequential?",
            "timestamps: utc iso or epoch?",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let questions: Vec<ClarifyQuestion> = decisions.iter().take(3).map(|d| q(d)).collect();
        let sink = ValueSink::default();
        let out =
            ask_clarifying_questions(&questions, dir.path(), None, &decisions, 0, None, &sink)
                .await;
        assert!(out.is_empty(), "nothing answered; got {out:?}");
        // Scoped so the guard provably drops before the next .await (await_holding_lock).
        {
            let events = sink.0.lock().unwrap();
            let ask = events
                .iter()
                .find(|e| e.get("event").and_then(|v| v.as_str()) == Some("low_confidence_ask"))
                .expect("the ask event is written");
            assert_eq!(
                ask.get("open_decisions_total").and_then(|v| v.as_u64()),
                Some(5),
                "the real count, not the dead-arg 0"
            );
            assert_eq!(
                ask.get("open_decisions_not_asked").and_then(|v| v.as_u64()),
                Some(2)
            );
            let named: Vec<&str> = ask["open_decisions_not_asked_detail"]
                .as_array()
                .expect("the guessed decisions are named, never only counted")
                .iter()
                .filter_map(|v| v.as_str())
                .collect();
            assert_eq!(
                named,
                vec!["ids: uuid or sequential?", "timestamps: utc iso or epoch?"],
                "verbatim, so no one has to diff by hand"
            );
        }
        // And when every decision is asked — the live planner path since the truncation kill —
        // the event says 0 guessed and carries no detail key.
        let all_asked: Vec<ClarifyQuestion> = decisions.iter().map(|d| q(d)).collect();
        let sink2 = ValueSink::default();
        let _ = ask_clarifying_questions(&all_asked, dir.path(), None, &decisions, 0, None, &sink2)
            .await;
        let events2 = sink2.0.lock().unwrap();
        let ask2 = events2
            .iter()
            .find(|e| e.get("event").and_then(|v| v.as_str()) == Some("low_confidence_ask"))
            .expect("the ask event is written");
        assert_eq!(
            ask2.get("open_decisions_not_asked")
                .and_then(|v| v.as_u64()),
            Some(0)
        );
        assert!(
            ask2.get("open_decisions_not_asked_detail").is_none(),
            "no absent-decision detail when nothing was dropped"
        );
    }

    #[test]
    fn clarify_question_resolves_optional_and_omitted_when_empty() {
        // Old JSON without `resolves` still deserializes (default empty).
        let q: ClarifyQuestion =
            serde_json::from_str(r#"{"question":"Which DB?","options":["SQLite"]}"#).unwrap();
        assert_eq!(q.resolves, "");
        // Empty resolves is omitted on serialize (byte-identical wire for old consumers).
        let s = serde_json::to_string(&q).unwrap();
        assert!(
            !s.contains("resolves"),
            "empty resolves must be omitted: {s}"
        );
        // Non-empty resolves round-trips.
        let q2 = ClarifyQuestion {
            question: "Which DB?".into(),
            options: vec![],
            resolves: "storage backend".into(),
        };
        let s2 = serde_json::to_string(&q2).unwrap();
        assert!(s2.contains("\"resolves\":\"storage backend\""));
    }

    /// r6e E10: a qualified two-option decision reaches the ASK payload with BOTH options — in the
    /// event the tick reads and in the file the harness/desktop reads — never `options: []`.
    #[tokio::test]
    async fn a_qualified_decisions_options_reach_the_ask_payload() {
        let d = OpenDecision {
            line: "HTTP framework for ledgerd — options: Flask | FastAPI".to_string(),
            options: vec!["Flask".to_string(), "FastAPI".to_string()],
        };
        let qs = clarify_questions(std::slice::from_ref(&d));
        assert_eq!(qs.len(), 1);
        assert_eq!(qs[0].question, d.line);
        assert_eq!(qs[0].options, vec!["Flask", "FastAPI"]);
        #[derive(Default)]
        struct AskSink(Mutex<Vec<serde_json::Value>>);
        impl EventSink for AskSink {
            fn emit(&self, _e: &SwarmEvent) {}
            fn write_value(&self, v: serde_json::Value) {
                self.0.lock().unwrap().push(v);
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let sink = AskSink::default();
        let lines = vec![d.line.clone()];
        let _ = ask_clarifying_questions(&qs, dir.path(), None, &lines, 0, None, &sink).await;
        let ask = sink.0.lock().unwrap()[0].clone();
        assert_eq!(ask["event"], "low_confidence_ask");
        assert_eq!(
            ask["questions"][0]["options"],
            serde_json::json!(["Flask", "FastAPI"])
        );
        let file: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join(".swarm/clarify-questions.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            file["questions"][0]["options"],
            serde_json::json!(["Flask", "FastAPI"])
        );
    }
}
