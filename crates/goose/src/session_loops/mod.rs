//! Session loops (Q-228; design local-edition/mlx/quality/DESIGN-SESSION-LOOPS.md).
//!
//! A session loop belongs to one chat: it re-runs the user's goal, tick after tick, in the same
//! session. One tick is one ordinary agent turn sent through the same prompt door a typed message
//! uses. This module is L0: the record (`extension_data["loop.v0"]`), the pure rules the runner and
//! the desktop both follow (pinned by `loops.fixture.json`, which the desktop's suite runs too), the
//! tick prompt, the templates, the bodies of the `loops/*` ACP methods, and the seam through which
//! the runner (L2a) and the report tool's extension sync (L3) are reached — with nothing installed,
//! every mutation answers the NAMED refusal "The loop runner is not in this build".
//!
//! Nothing here decides model work by time: a cadence says only when a tick STARTS, and nothing
//! cuts a tick or a check.

pub mod acp;
pub mod agent_sync;
pub mod prompt;
pub mod record;
pub mod rules;
pub mod seam;
pub mod templates;

#[cfg(test)]
mod tests;

/// Every tick's prompt message id starts with this (`looptick_<loopId>_<n>_<uuid>`).
pub const TICK_ID_PREFIX: &str = "looptick_";
/// Every loop id starts with this (`lp_<8 hex>`).
pub const LOOP_ID_PREFIX: &str = "lp_";
