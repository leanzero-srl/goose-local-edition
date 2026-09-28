//! Q-357: what the person set for a chat's compactions and how the last one went, durable in the
//! session's `extension_data` (`compaction.v0`). Entry points: the meter menu, `/compact <note>`, the
//! Context tab. A note is used once and cleared; a standing note stays; pins stay.

use anyhow::Result;
use chrono::{DateTime, Utc};
use goose_sdk_types::custom_notifications::{CompactionNoteVerdict, CompactionTriggerKind};
use goose_sdk_types::custom_requests::{CompactionSteerDto, LastCompactionDto};
use serde::{Deserialize, Serialize};

use crate::session::extension_data::{ExtensionData, ExtensionState};
use crate::session::SessionManager;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default)]
    pub standing: bool,
    #[serde(default)]
    pub pins: Vec<String>,
    #[serde(default)]
    pub follow_as_written: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<LastCompaction>,
}

impl ExtensionState for CompactionState {
    const EXTENSION_NAME: &'static str = "compaction";
    const VERSION: &'static str = "v0";
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastCompaction {
    pub at: DateTime<Utc>,
    pub trigger: CompactionTriggerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_after: Option<u64>,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_verdict: Option<CompactionNoteVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub said: Option<String>,
}

impl CompactionState {
    /// The chat's record: absent = nothing set yet; a record that does not parse is an error the
    /// caller says, never an empty record.
    pub fn of(extension_data: &ExtensionData) -> Result<Self> {
        match extension_data.get_extension_state(Self::EXTENSION_NAME, Self::VERSION) {
            None => Ok(Self::default()),
            Some(value) => Self::from_value(value),
        }
    }

    pub async fn load(sessions: &SessionManager, session_id: &str) -> Result<Self> {
        let session = sessions.get_session(session_id, false).await?;
        Self::of(&session.extension_data)
    }

    pub async fn save(&self, sessions: &SessionManager, session_id: &str) -> Result<()> {
        sessions.set_extension_state(session_id, self).await
    }

    /// The note, trimmed, when there is one.
    pub fn note(&self) -> Option<&str> {
        self.note
            .as_deref()
            .map(str::trim)
            .filter(|note| !note.is_empty())
    }

    /// After a compaction that summarized the conversation: a one-time note is spent, a
    /// "compact as written" answer is used up, and the compaction is the last one.
    pub fn after_compaction(mut self, last: LastCompaction) -> Self {
        if !self.standing {
            self.note = None;
        }
        self.follow_as_written = false;
        self.last = Some(last);
        self
    }

    pub fn steer(&self) -> CompactionSteerDto {
        CompactionSteerDto {
            note: self.note.clone(),
            standing: self.standing,
            pins: self.pins.clone(),
            follow_as_written: self.follow_as_written,
        }
    }

    /// The person's settings replaced; the last compaction stays.
    pub fn with_steer(mut self, steer: CompactionSteerDto) -> Self {
        self.note = steer
            .note
            .map(|note| note.trim().to_string())
            .filter(|note| !note.is_empty());
        self.standing = steer.standing;
        self.pins = steer
            .pins
            .into_iter()
            .map(|pin| pin.trim().to_string())
            .filter(|pin| !pin.is_empty())
            .collect();
        self.follow_as_written = steer.follow_as_written;
        self
    }
}

impl From<&LastCompaction> for LastCompactionDto {
    fn from(last: &LastCompaction) -> Self {
        Self {
            at: last.at.to_rfc3339(),
            trigger: last.trigger,
            tokens_before: last.tokens_before,
            tokens_after: last.tokens_after,
            elapsed_ms: last.elapsed_ms,
            note_verdict: last.note_verdict,
            said: last.said.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn last() -> LastCompaction {
        LastCompaction {
            at: Utc::now(),
            trigger: CompactionTriggerKind::Auto,
            tokens_before: Some(142_100),
            tokens_after: Some(6_300),
            elapsed_ms: 238_000,
            note_verdict: None,
            said: None,
        }
    }

    #[test]
    fn a_one_time_note_is_spent_and_a_standing_one_stays() {
        let once = CompactionState {
            note: Some("keep the 24-month rule".to_string()),
            follow_as_written: true,
            pins: vec!["seed 20260928".to_string()],
            ..Default::default()
        }
        .after_compaction(last());
        assert_eq!(once.note, None);
        assert!(!once.follow_as_written);
        assert_eq!(once.pins, ["seed 20260928"]);
        assert!(once.last.is_some());

        let standing = CompactionState {
            note: Some("keep the 24-month rule".to_string()),
            standing: true,
            ..Default::default()
        }
        .after_compaction(last());
        assert_eq!(standing.note(), Some("keep the 24-month rule"));
    }

    #[test]
    fn an_unreadable_record_is_an_error_not_an_empty_one() {
        let mut data = ExtensionData::new();
        data.set_extension_state(
            "compaction",
            "v0",
            serde_json::json!({"pins": "not a list"}),
        );
        assert!(CompactionState::of(&data).is_err());
        assert_eq!(
            CompactionState::of(&ExtensionData::new()).unwrap(),
            CompactionState::default()
        );
    }

    #[test]
    fn the_steer_is_replaced_trimmed_and_the_last_compaction_kept() {
        let state = CompactionState {
            last: Some(last()),
            ..Default::default()
        }
        .with_steer(CompactionSteerDto {
            note: Some("   ".to_string()),
            standing: true,
            pins: vec![" seed 20260928 ".to_string(), "".to_string()],
            follow_as_written: false,
        });
        assert_eq!(state.note, None);
        assert_eq!(state.pins, ["seed 20260928"]);
        assert!(state.last.is_some());
    }
}
