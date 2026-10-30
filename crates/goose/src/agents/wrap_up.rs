//! The wrap-up nudge: goose's own words in the turn-context block when a session nears its call budget.
//!
//! WHY: a budgeted session only saw `<turn-budget>T/M used</turn-budget>` past half its budget, and the
//! task text says "plan to finish inside it". Nothing told the model to stop building, so runs ended
//! mid-polish with required outputs never written: 20 board runs hit the budget, and the local Flash
//! sb-7.2 run of 2026-10-30 still had DECISIONS.md and faults.py open at about 125 of 150 calls.
//!
//! WHAT: three stages, each shown once when it is crossed (a chat keeps its blocks in the history, so the
//! line stays visible to every later request; a swarm worker's block is rebuilt per request, so it
//! carries the current stage every time):
//!   finish  (at [`FINISH_AT_DEFAULT`] of the budget): start no new feature, finish what is in progress;
//!   deliver (at [`DELIVER_AT_DEFAULT`]): make it run, write the required outputs, leave it finished;
//!   final   (the last [`FINAL_CALLS_DEFAULT`] calls): verify it runs, no new work.
//! The required outputs are named from the request itself: the backticked file names the person's
//! request contains that do not exist in the working directory yet (the SB7.2 prompt names
//! `DECISIONS.md`). With none, the line names none — no generic task text (AGENTS.md gate 2).
//!
//! COUNT: the reply loop's `turns_taken`, the number `--max-turns` stops at (agent.rs: `turns_taken >
//! max_turns` ends the session). A resend after an empty reply or a transient error is not a turn, so it
//! neither moves the cap nor this nudge; the harness's `calls_used` (telemetry) can exceed the cap by
//! exactly those resends, which is why the nudge counts turns, not telemetry calls (bench_budget.py).
//!
//! Every threshold and text is a tunable (config key or env var). Swarm workers are OFF by default:
//! their request bodies are the golden engine's (AGENTS.md: an engine change lands only with a measured
//! run), so [`WRAP_UP_SWARM_WORKERS_KEY`] arms them explicitly.

use crate::config::Config;
use crate::conversation::message::{Message, MessageContent};
use crate::conversation::{effective_role, is_turn_context_text};
use std::path::Path;

/// Bumped whenever a default threshold or text changes, so a benchmark run records which nudge it ran
/// with (bench_budget.py WRAP_UP_NUDGE_VERSION is pinned to this by test_bench_budget).
pub const WRAP_UP_NUDGE_VERSION: u32 = 2;

/// Master switch (default on): chats and single-model benchmark entrants.
pub const WRAP_UP_NUDGE_KEY: &str = "GOOSE_WRAP_UP_NUDGE";
/// Swarm workers (default off: the golden engine's request bodies stay byte-identical).
pub const WRAP_UP_SWARM_WORKERS_KEY: &str = "GOOSE_WRAP_UP_SWARM_WORKERS";
pub const FINISH_AT_KEY: &str = "GOOSE_WRAP_UP_FINISH_AT";
pub const DELIVER_AT_KEY: &str = "GOOSE_WRAP_UP_DELIVER_AT";
pub const FINAL_CALLS_KEY: &str = "GOOSE_WRAP_UP_FINAL_CALLS";
pub const FINISH_TEXT_KEY: &str = "GOOSE_WRAP_UP_TEXT_FINISH";
pub const DELIVER_TEXT_KEY: &str = "GOOSE_WRAP_UP_TEXT_DELIVER";
pub const FINAL_TEXT_KEY: &str = "GOOSE_WRAP_UP_TEXT_FINAL";

/// policy (owner, 2026-10-30): "at ~80%: stop starting new features; finish what's in progress".
pub const FINISH_AT_DEFAULT: f64 = 0.8;
/// policy (owner, 2026-10-30): "at ~90%: make sure the app runs, write the required deliverables".
pub const DELIVER_AT_DEFAULT: f64 = 0.9;
/// policy (owner, 2026-10-30): "at the last ~3 calls: verify it runs; no new work".
pub const FINAL_CALLS_DEFAULT: u32 = 3;

/// Placeholders: `{left}` calls after this one, `{max}` the budget, `{outputs}` the sentence naming the
/// request's files not written yet (empty when there are none).
pub const FINISH_TEXT_DEFAULT: &str =
    "{left} of {max} model calls left after this one. Start no new \
feature: finish and test the work already in progress.{outputs}";
pub const DELIVER_TEXT_DEFAULT: &str =
    "{left} of {max} model calls left after this one. Make sure what \
you built runs, write the outputs the request requires, and leave the workspace in a finished \
state.{outputs}";
pub const FINAL_TEXT_DEFAULT: &str =
    "Final calls of the {max}-call budget: this one and {left} more. \
Verify that it runs; start no new work.{outputs}";

/// Fractions are compared in thousandths, so 0.9 × 150 is 135 and not 135.00000000000003.
const PER_MILLE: u64 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Finish,
    Deliver,
    Final,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Stage::Finish => "finish",
            Stage::Deliver => "deliver",
            Stage::Final => "final",
        }
    }

    /// The line's opening, which also identifies a stage already shown.
    fn opening(self) -> String {
        format!("<wrap-up stage=\"{}\">", self.name())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WrapUp {
    finish_per_mille: u64,
    deliver_per_mille: u64,
    final_calls: u32,
    finish_text: String,
    deliver_text: String,
    final_text: String,
}

impl Default for WrapUp {
    fn default() -> Self {
        Self::new(
            FINISH_AT_DEFAULT,
            DELIVER_AT_DEFAULT,
            FINAL_CALLS_DEFAULT,
            [
                FINISH_TEXT_DEFAULT,
                DELIVER_TEXT_DEFAULT,
                FINAL_TEXT_DEFAULT,
            ]
            .map(String::from),
        )
    }
}

fn per_mille(fraction: f64) -> u64 {
    (fraction * PER_MILLE as f64).round() as u64
}

impl WrapUp {
    pub fn new(finish_at: f64, deliver_at: f64, final_calls: u32, texts: [String; 3]) -> Self {
        let [finish_text, deliver_text, final_text] = texts;
        Self {
            finish_per_mille: per_mille(finish_at),
            deliver_per_mille: per_mille(deliver_at),
            final_calls,
            finish_text,
            deliver_text,
            final_text,
        }
    }

    /// The nudge this agent runs with, or `None` when it is off for it. A threshold outside (0, 1], or a
    /// deliver point before the finish point, is refused loudly and the default is used.
    pub fn from_config(config: &Config, swarm_worker: bool) -> Option<Self> {
        let key = if swarm_worker {
            WRAP_UP_SWARM_WORKERS_KEY
        } else {
            WRAP_UP_NUDGE_KEY
        };
        if !config.get_param::<bool>(key).unwrap_or(!swarm_worker) {
            return None;
        }
        let fraction = |key: &str, default: f64| {
            let value = config.get_param::<f64>(key).unwrap_or(default);
            if value > 0.0 && value <= 1.0 {
                value
            } else {
                tracing::warn!(
                    key,
                    value,
                    default,
                    "wrap-up nudge: not a fraction in (0, 1]; using the default"
                );
                default
            }
        };
        let mut finish_at = fraction(FINISH_AT_KEY, FINISH_AT_DEFAULT);
        let mut deliver_at = fraction(DELIVER_AT_KEY, DELIVER_AT_DEFAULT);
        if deliver_at < finish_at {
            tracing::warn!(
                finish_at,
                deliver_at,
                "wrap-up nudge: deliver point before the finish point; using the defaults"
            );
            (finish_at, deliver_at) = (FINISH_AT_DEFAULT, DELIVER_AT_DEFAULT);
        }
        let text = |key: &str, default: &str| {
            config
                .get_param::<String>(key)
                .ok()
                .filter(|text| !text.trim().is_empty())
                .unwrap_or_else(|| default.to_string())
        };
        Some(Self::new(
            finish_at,
            deliver_at,
            config
                .get_param::<u32>(FINAL_CALLS_KEY)
                .unwrap_or(FINAL_CALLS_DEFAULT),
            [
                text(FINISH_TEXT_KEY, FINISH_TEXT_DEFAULT),
                text(DELIVER_TEXT_KEY, DELIVER_TEXT_DEFAULT),
                text(FINAL_TEXT_KEY, FINAL_TEXT_DEFAULT),
            ],
        ))
    }

    /// The stage of the call `turns_taken` (already counting this call, as `<turn-budget>` does).
    pub fn stage(&self, turns_taken: u32, max_turns: u32) -> Option<Stage> {
        if max_turns == 0 || turns_taken == 0 || turns_taken > max_turns {
            return None;
        }
        let (taken, max) = (u64::from(turns_taken), u64::from(max_turns));
        if max_turns - turns_taken < self.final_calls {
            Some(Stage::Final)
        } else if taken * PER_MILLE >= self.deliver_per_mille * max {
            Some(Stage::Deliver)
        } else if taken * PER_MILLE >= self.finish_per_mille * max {
            Some(Stage::Finish)
        } else {
            None
        }
    }

    /// The `<wrap-up>` line for `stage`, one line (the block's head ends at its first blank line).
    pub fn line(
        &self,
        stage: Stage,
        turns_taken: u32,
        max_turns: u32,
        missing: &[String],
    ) -> String {
        let template = match stage {
            Stage::Finish => &self.finish_text,
            Stage::Deliver => &self.deliver_text,
            Stage::Final => &self.final_text,
        };
        let outputs = if missing.is_empty() {
            String::new()
        } else {
            // v2 (A/B 2026-10-30, glm-5.3, budget 30): v1's finish lead ("Not written yet...") read as
            // information; a run told at call 24 that five named files were missing kept debugging
            // for three calls, then wrote one file per call and ran out before DECISIONS.md. A
            // required output is not a new feature, so every stage now puts the missing files first.
            let lead = match stage {
                Stage::Finish => {
                    "Write these first, before more fixing: the request names them and they do not exist yet"
                }
                Stage::Deliver | Stage::Final => {
                    "Write these now, before anything else: the request names them and they do not exist yet"
                }
            };
            // The same run said "writing the three test files plus README and DECISIONS in one go"
            // and still sent one write per reply: say that one reply can carry several writes.
            let together = if missing.len() > 1 {
                " One reply can write several of them: send one write call per file in the same reply."
            } else {
                ""
            };
            format!(" {lead}: {}.{together}", missing.join(", "))
        };
        let text = template
            .replace("{left}", &max_turns.saturating_sub(turns_taken).to_string())
            .replace("{max}", &max_turns.to_string())
            .replace("{outputs}", &outputs);
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        format!(
            "{}{}</wrap-up>",
            stage.opening(),
            text.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        )
    }

    /// The line this request carries, or `None`. `messages` is the history the block will join;
    /// `once` (a chat) leaves out a stage already shown since the person last wrote.
    pub fn nudge(
        &self,
        turns_taken: u32,
        max_turns: u32,
        messages: &[Message],
        working_dir: &str,
        once: bool,
    ) -> Option<String> {
        let stage = self.stage(turns_taken, max_turns)?;
        if once && shown_since_persons_words(messages, stage) {
            return None;
        }
        let missing = missing_outputs(&named_files(messages), Path::new(working_dir));
        Some(self.line(stage, turns_taken, max_turns, &missing))
    }
}

fn persons_text(message: &Message) -> Option<String> {
    let person = message.is_agent_visible()
        && message.is_user_visible()
        && effective_role(message) == "user";
    if !person {
        return None;
    }
    let text: Vec<&str> = message
        .content
        .iter()
        .filter_map(MessageContent::as_text)
        .filter(|text| !text.trim().is_empty() && !is_turn_context_text(text))
        .collect();
    (!text.is_empty()).then(|| text.join("\n"))
}

fn last_persons_words(messages: &[Message]) -> Option<usize> {
    messages.iter().rposition(|m| persons_text(m).is_some())
}

fn shown_since_persons_words(messages: &[Message], stage: Stage) -> bool {
    let from = last_persons_words(messages).unwrap_or(0);
    let opening = stage.opening();
    messages[from..].iter().any(|m| {
        m.is_agent_visible()
            && m.content
                .iter()
                .filter_map(MessageContent::as_text)
                .any(|text| is_turn_context_text(text) && text.contains(&opening))
    })
}

/// The backticked file names in the person's latest request, in order, once each. A file name is a
/// path of letters, digits, `_`, `-`, `.` and `/`, with a letter somewhere and an extension of one to
/// eight letters or digits — `DECISIONS.md`, `app/faults.py`; not `vs7dbg`, `0.5` or a URL.
pub fn named_files(messages: &[Message]) -> Vec<String> {
    let Some(text) = last_persons_words(messages).and_then(|at| persons_text(&messages[at])) else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    for token in text.split('`').skip(1).step_by(2) {
        if is_file_name(token) && !names.iter().any(|name| name == token) {
            names.push(token.to_string());
        }
    }
    names
}

fn is_file_name(token: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/');
    let Some((stem, extension)) = token.rsplit_once('.') else {
        return false;
    };
    !token.is_empty()
        && token.chars().all(allowed)
        && !token.contains("//")
        && stem.chars().any(|c| c.is_ascii_alphabetic())
        && !stem.ends_with('/')
        && (1..=8).contains(&extension.len())
        && extension.chars().all(|c| c.is_ascii_alphanumeric())
        && extension.chars().any(|c| c.is_ascii_alphabetic())
}

fn missing_outputs(named: &[String], working_dir: &Path) -> Vec<String> {
    named
        .iter()
        .filter(|name| !working_dir.join(name).exists())
        .cloned()
        .collect()
}

/// `block` with `line` added as the last line of its head (before its first blank line or its close).
pub fn with_line(block: &str, line: &str) -> String {
    let close = format!("</{}>", crate::conversation::TURN_CONTEXT_TAG);
    let lines: Vec<&str> = block.split('\n').collect();
    let at = lines
        .iter()
        .position(|l| l.is_empty())
        .or_else(|| lines.iter().rposition(|l| *l == close))
        .unwrap_or(lines.len());
    let mut out = lines;
    out.insert(at, line);
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stages(budget: u32) -> Vec<(u32, Stage)> {
        let wrap = WrapUp::default();
        let mut crossings = Vec::new();
        let mut last = None;
        for turn in 1..=budget {
            let stage = wrap.stage(turn, budget);
            if stage != last {
                crossings.push((turn, stage.expect("a stage never ends before the budget")));
                last = stage;
            }
        }
        crossings
    }

    #[test]
    fn stages_cross_at_the_owners_points_for_150_300_and_1000() {
        assert_eq!(
            stages(150),
            [
                (120, Stage::Finish),
                (135, Stage::Deliver),
                (148, Stage::Final)
            ]
        );
        assert_eq!(
            stages(300),
            [
                (240, Stage::Finish),
                (270, Stage::Deliver),
                (298, Stage::Final)
            ]
        );
        assert_eq!(
            stages(1000),
            [
                (800, Stage::Finish),
                (900, Stage::Deliver),
                (998, Stage::Final)
            ]
        );
        // The probe's budget: all three stages still show.
        assert_eq!(
            stages(30),
            [
                (24, Stage::Finish),
                (27, Stage::Deliver),
                (28, Stage::Final)
            ]
        );
    }

    #[test]
    fn no_stage_without_a_budget_or_past_it() {
        let wrap = WrapUp::default();
        assert_eq!(wrap.stage(5, 0), None);
        assert_eq!(wrap.stage(151, 150), None);
        assert_eq!(wrap.stage(119, 150), None);
    }

    #[test]
    fn the_line_counts_what_the_turn_budget_counts_and_names_missing_outputs() {
        let wrap = WrapUp::default();
        let line = wrap.line(Stage::Deliver, 135, 150, &["DECISIONS.md".to_string()]);
        assert_eq!(
            line,
            "<wrap-up stage=\"deliver\">15 of 150 model calls left after this one. Make sure what \
             you built runs, write the outputs the request requires, and leave the workspace in a \
             finished state. Write these now, before anything else: the request names them and they do \
             not exist yet: DECISIONS.md.</wrap-up>"
        );
        let finish = wrap.line(
            Stage::Finish,
            120,
            150,
            &["README.md".to_string(), "DECISIONS.md".to_string()],
        );
        assert!(
            finish.contains(
                "Write these first, before more fixing: the request names them and they do not \
                 exist yet: README.md, DECISIONS.md. One reply can write several of them"
            ),
            "{finish}"
        );
        let last = wrap.line(Stage::Final, 150, 150, &[]);
        assert!(last.contains("this one and 0 more"), "{last}");
        assert!(
            !last.contains("{outputs}") && !last.contains("do not exist"),
            "{last}"
        );
    }

    #[test]
    fn named_files_are_the_backticked_file_names_of_the_latest_request() {
        let prompt = "Read `SB7-CONTRACT.md` and `STARTER.md`. 10. `DECISIONS.md` documents D1. \
                      A truthful `vs7dbg`, `0.5`, `npm run lint`, `https://x.io/a.js`, `app/faults.py`, \
                      `DECISIONS.md` again.";
        let messages = vec![
            Message::user().with_text("`old.md`"),
            Message::assistant().with_text("ok"),
            Message::user().with_text(prompt),
        ];
        assert_eq!(
            named_files(&messages),
            [
                "SB7-CONTRACT.md",
                "STARTER.md",
                "DECISIONS.md",
                "app/faults.py"
            ]
        );
    }

    #[test]
    fn only_files_not_on_disk_are_named() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("STARTER.md"), "x").unwrap();
        let named = vec!["STARTER.md".to_string(), "DECISIONS.md".to_string()];
        assert_eq!(missing_outputs(&named, dir.path()), ["DECISIONS.md"]);
    }

    #[test]
    fn a_chat_shows_each_stage_once_after_the_persons_words() {
        let wrap = WrapUp::default();
        let block = |line: &str| {
            format!(
                "<turn-context>\n<current-time>t</current-time>\n<working-directory>/w</working-directory>\n{line}\n</turn-context>"
            )
        };
        let shown = wrap.line(Stage::Finish, 120, 150, &[]);
        let mut history = vec![
            Message::user().with_text("build it"),
            Message::user().with_text(block(&shown)).agent_only(),
        ];
        assert_eq!(wrap.nudge(121, 150, &history, "/nonexistent", true), None);
        assert!(wrap
            .nudge(121, 150, &history, "/nonexistent", false)
            .is_some());
        assert!(wrap
            .nudge(135, 150, &history, "/nonexistent", true)
            .is_some());
        // A new request from the person resets what was shown.
        history.push(Message::user().with_text("now the next thing"));
        assert!(wrap
            .nudge(121, 150, &history, "/nonexistent", true)
            .is_some());
    }

    #[test]
    fn the_line_joins_the_head_of_the_block() {
        let block = "<turn-context>\n<current-time>t</current-time>\n<turn-budget>1/2 used</turn-budget>\n\n<ext>x</ext>\n</turn-context>";
        assert_eq!(
            with_line(block, "<wrap-up stage=\"final\">v</wrap-up>"),
            "<turn-context>\n<current-time>t</current-time>\n<turn-budget>1/2 used</turn-budget>\n<wrap-up stage=\"final\">v</wrap-up>\n\n<ext>x</ext>\n</turn-context>"
        );
        let bare = "<turn-context>\n<current-time>t</current-time>\n</turn-context>";
        assert_eq!(
            with_line(bare, "L"),
            "<turn-context>\n<current-time>t</current-time>\nL\n</turn-context>"
        );
    }
    fn config_with(yaml: &str) -> (tempfile::TempDir, Config) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, yaml).unwrap();
        let config = Config::new_with_file_secrets(&path, dir.path().join("secrets.yaml")).unwrap();
        (dir, config)
    }

    #[test]
    fn chats_are_on_and_swarm_workers_off_unless_configured() {
        let (_dir, config) = config_with("{}\n");
        assert_eq!(WrapUp::from_config(&config, false), Some(WrapUp::default()));
        assert_eq!(WrapUp::from_config(&config, true), None);

        let (_dir, config) =
            config_with("GOOSE_WRAP_UP_NUDGE: false\nGOOSE_WRAP_UP_SWARM_WORKERS: true\n");
        assert_eq!(WrapUp::from_config(&config, false), None);
        assert_eq!(WrapUp::from_config(&config, true), Some(WrapUp::default()));
    }

    #[test]
    fn thresholds_and_texts_are_tunable_and_bad_values_fall_back_loudly() {
        let (_dir, config) = config_with(
            "GOOSE_WRAP_UP_FINISH_AT: 0.5\nGOOSE_WRAP_UP_DELIVER_AT: 0.75\nGOOSE_WRAP_UP_FINAL_CALLS: 1\nGOOSE_WRAP_UP_TEXT_FINAL: \"last: {left}\"\n",
        );
        let wrap = WrapUp::from_config(&config, false).unwrap();
        assert_eq!(wrap.stage(50, 100), Some(Stage::Finish));
        assert_eq!(wrap.stage(75, 100), Some(Stage::Deliver));
        assert_eq!(wrap.stage(99, 100), Some(Stage::Deliver));
        assert_eq!(wrap.stage(100, 100), Some(Stage::Final));
        assert_eq!(
            wrap.line(Stage::Final, 100, 100, &[]),
            "<wrap-up stage=\"final\">last: 0</wrap-up>"
        );

        let (_dir, config) =
            config_with("GOOSE_WRAP_UP_FINISH_AT: 1.5\nGOOSE_WRAP_UP_DELIVER_AT: 0.2\n");
        assert_eq!(WrapUp::from_config(&config, false), Some(WrapUp::default()));
    }
}
