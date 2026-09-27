//! The four starting templates (design §7.4), served by `loops/templates`. The steps are
//! parameterised by THIS loop's facts through slots (`{state_file}`, `{check}`, …) that
//! `rules::render_steps` fills from the record at every tick, so an edit to the check or the state
//! file reaches the next tick without re-typing the steps. The user sees the rendered steps in the
//! Start dialog and can edit them before any model reads them; the record keeps the text as the
//! user left it, slots included. A slot renders its fact quoted (paths and commands in backticks,
//! words in double quotes), so the template text never quotes a slot itself.

use goose_sdk_types::custom_requests::{LoopCadence, LoopTemplateDto, LoopTemplateId};

pub const SLOT_STATE_FILE: &str = "state_file";
pub const SLOT_CHECK: &str = "check";
pub const SLOT_GOAL_FIRST_LINE: &str = "goal_first_line";
pub const SLOT_LAST_NEXT_STEP: &str = "last_next_step";
pub const SLOT_WORKING_DIR: &str = "working_dir";

/// Every fact a step may name.
pub const SLOTS: [&str; 5] = [
    SLOT_STATE_FILE,
    SLOT_CHECK,
    SLOT_GOAL_FIRST_LINE,
    SLOT_LAST_NEXT_STEP,
    SLOT_WORKING_DIR,
];

const QUALITY_STEPS: &str = "1. Discover: open {state_file}, then run or read what {goal_first_line} names in {working_dir}. List what is broken, missing or confusing, each with the evidence you saw (command output, file:line).
2. Critique: rank what you found by how much it blocks the goal; pick the ONE item that matters most ({last_next_step}).
3. Fix: make that change, and only that change.
4. Prove it. Check to run: {check}. A fix without a quoted result is not done.
5. Rewrite {state_file}: what is now true, what is next, what you found but did not fix.";

const UNTIL_CHECK_STEPS: &str = "1. Run the check — {check} — and read why it fails.
2. Fix the first cause it names.
3. Run the check again — {check} — and quote the result.
4. Rewrite {state_file}.";

const WATCH_STEPS: &str = "1. Look at what {goal_first_line} names (a build, a deploy, a folder, a URL) and compare it with {state_file}.
2. If nothing changed, say so in one line and report progress.
3. If something changed, do what the goal asks and quote the evidence.
4. Rewrite {state_file}.";

pub fn template(id: LoopTemplateId) -> LoopTemplateDto {
    let (name, description, steps, suggested_cadence, needs_check) = match id {
        LoopTemplateId::Quality => (
            "Software quality loop",
            "Discover what's broken, pick the one thing that matters most, fix it, prove it with a check. Repeat.",
            QUALITY_STEPS,
            LoopCadence::Every {
                every: "10m".to_string(),
            },
            false,
        ),
        LoopTemplateId::UntilCheck => (
            "Until a check passes",
            "Keep working until a command you name succeeds — tests, a build, a lint.",
            UNTIL_CHECK_STEPS,
            LoopCadence::BackToBack,
            true,
        ),
        LoopTemplateId::Watch => (
            "Watch and act",
            "Look at something on a schedule and act when it changes — a build, a deploy, a folder.",
            WATCH_STEPS,
            LoopCadence::Every {
                every: "30m".to_string(),
            },
            false,
        ),
        LoopTemplateId::Blank => (
            "Blank",
            "Your goal, your steps.",
            "",
            LoopCadence::SelfPaced,
            false,
        ),
    };
    LoopTemplateDto {
        id,
        name: name.to_string(),
        description: description.to_string(),
        steps: steps.to_string(),
        slots: super::rules::step_slots(steps),
        suggested_cadence,
        needs_check,
    }
}

pub fn all() -> Vec<LoopTemplateDto> {
    [
        LoopTemplateId::Quality,
        LoopTemplateId::UntilCheck,
        LoopTemplateId::Watch,
        LoopTemplateId::Blank,
    ]
    .into_iter()
    .map(template)
    .collect()
}
