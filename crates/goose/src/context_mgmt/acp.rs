//! Q-357: the desktop's doors to a chat's compaction — the preview the Context tab and the meter
//! menu read (code only, no model call) and the person's note and pins.

use agent_client_protocol::Error as AcpError;
use goose_sdk_types::custom_requests::{
    AlwaysHereDto, CompactionPreviewRequest, CompactionPreviewResponse, CompactionSteerRequest,
    CompactionSteerResponse, KeptPillarDto, KeptPillarId, LastCompactionDto, WrittenPartDto,
};

use super::pillars::{KeptSources, LedgerRead, Pillar, Pillars};
use super::state::CompactionState;
use super::{kept_budget_chars, latest_summary, WRITTEN_PARTS};
use crate::agents::platform_extensions::ledger::{self, LedgerFile};
use crate::agents::Agent;
use crate::session::extension_data::{ExtensionState, TodoState};
use crate::session::SessionManager;

fn internal(e: impl std::fmt::Display) -> AcpError {
    AcpError::internal_error().data(e.to_string())
}

fn pillar_id(pillar: Pillar) -> KeptPillarId {
    match pillar {
        Pillar::Asked => KeptPillarId::Asked,
        Pillar::Files => KeptPillarId::Files,
        Pillar::Failed => KeptPillarId::Failed,
        Pillar::Notes => KeptPillarId::Notes,
        Pillar::Ledger => KeptPillarId::Ledger,
    }
}

/// What a compaction of this chat would keep now, as a manual compaction would (the newest message
/// is kept with the others: nothing follows the summary). The window is the agent's; a chat whose
/// provider cannot say it answers no budget, and nothing is cut.
pub async fn preview(
    agent: &Agent,
    sessions: &SessionManager,
    req: CompactionPreviewRequest,
) -> Result<CompactionPreviewResponse, AcpError> {
    let session = sessions
        .get_session(&req.session_id, true)
        .await
        .map_err(|e| AcpError::invalid_params().data(format!("Unknown session: {e}")))?;
    let (state, steer_error) = match CompactionState::of(&session.extension_data) {
        Ok(state) => (state, None),
        Err(e) => (CompactionState::default(), Some(e.to_string())),
    };
    let entries = LedgerFile::for_chat(
        &session.working_dir,
        &req.session_id,
        dirs::home_dir().as_deref(),
    )
    .read_all();
    let ledger_read = match &entries {
        Ok(entries) => LedgerRead::Entries(entries.clone()),
        Err(e) => LedgerRead::Unreadable(e.to_string()),
    };
    let window = match (
        agent.provider().await,
        agent.model_config_for_session(&req.session_id).await,
    ) {
        (Ok(provider), Ok(model_config)) => {
            super::effective_context_limit(provider.as_ref(), &model_config).await
        }
        _ => None,
    };
    let budget = kept_budget_chars(window);
    let messages = session
        .conversation
        .as_ref()
        .map(|c| c.messages().clone())
        .unwrap_or_default();
    let kept = Pillars::build(
        &messages,
        &KeptSources {
            note: state.note(),
            pins: &state.pins,
            preserved: None,
        },
        ledger_read,
    )
    .fit(budget);
    let counter = crate::token_counter::create_token_counter().await.ok();
    let pillars = Pillar::ALL
        .into_iter()
        .map(|pillar| {
            let section = kept.section(pillar);
            KeptPillarDto {
                id: pillar_id(pillar),
                items: kept.items(pillar),
                left_out: match pillar {
                    Pillar::Failed => kept.failed_left_out as u64,
                    Pillar::Ledger => kept.ledger_left_out as u64,
                    _ => 0,
                },
                cut: match pillar {
                    Pillar::Asked => kept.asked.iter().filter(|a| a.cut).count() as u64,
                    _ => 0,
                },
                tokens: counter
                    .as_ref()
                    .map(|counter| counter.count_tokens(&section) as u64),
                error: match (&kept.ledger, pillar) {
                    (LedgerRead::Unreadable(e), Pillar::Ledger) => Some(e.clone()),
                    _ => None,
                },
            }
        })
        .collect();
    Ok(CompactionPreviewResponse {
        kept: pillars,
        written_parts: WRITTEN_PARTS
            .iter()
            .map(|(heading, ask)| WrittenPartDto {
                heading: heading.to_string(),
                ask: ask.to_string(),
            })
            .collect(),
        always_here: AlwaysHereDto {
            scratchpad: TodoState::from_extension_data(&session.extension_data)
                .map(|todo| todo.content)
                .filter(|content| !content.trim().is_empty()),
            ledger_tail: entries.map(|e| ledger::tail(&e)).unwrap_or_default(),
        },
        steer: state.steer(),
        last: state.last.as_ref().map(LastCompactionDto::from),
        last_kept: latest_summary(&messages),
        kept_budget_tokens: window.map(|limit| (limit as f64 * super::KEPT_WINDOW_SHARE) as u64),
        steer_error,
    })
}

/// Replaces the person's note and pins for this chat's compactions.
pub async fn steer(
    sessions: &SessionManager,
    req: CompactionSteerRequest,
) -> Result<CompactionSteerResponse, AcpError> {
    let session = sessions
        .get_session(&req.session_id, false)
        .await
        .map_err(|e| AcpError::invalid_params().data(format!("Unknown session: {e}")))?;
    // An unreadable record is replaced by what the person set now: it is their note, and the
    // preview has already said the old one could not be read.
    let state = CompactionState::of(&session.extension_data)
        .unwrap_or_default()
        .with_steer(req.steer);
    state
        .save(sessions, &req.session_id)
        .await
        .map_err(internal)?;
    Ok(CompactionSteerResponse {
        steer: state.steer(),
    })
}
