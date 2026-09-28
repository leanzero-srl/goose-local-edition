//! The body of every `loops/*` ACP method (the `dispatch_loops_*` functions in
//! `acp/server/custom_dispatch.rs` call these and nothing else, beside the extension sync that
//! needs the session's agent). `get`, `list` and `templates` are pure reads implemented here;
//! every mutation validates, then goes to the runner through the seam — with no runner installed
//! it answers the named refusal and writes nothing.

use agent_client_protocol::Error as AcpError;
use goose_sdk_types::custom_requests::{
    LoopEdit, LoopRecord, LoopRefusal, LoopStatus, LoopStatusReason, LoopSummaryDto,
    LoopsChangeResponse, LoopsControlRequest, LoopsGetRequest, LoopsGetResponse, LoopsListResponse,
    LoopsReadyRequest, LoopsReadyResponse, LoopsStartRequest, LoopsTemplatesResponse,
    LoopsTickRefusedRequest, LoopsTickRefusedResponse, LoopsUpdateRequest, LoopsWakeResponse,
};

use super::rules::{self, ChatFacts, OwnerProof};
use super::{record, seam, templates};
use crate::nodes::{parse_route_model, RouteModel, SWARM_PROVIDER};
use crate::session::{Session, SessionManager};

fn internal(e: impl std::fmt::Display) -> AcpError {
    AcpError::internal_error().data(e.to_string())
}

async fn session(session_manager: &SessionManager, session_id: &str) -> Result<Session, AcpError> {
    session_manager
        .get_session(session_id, false)
        .await
        .map_err(|e| AcpError::invalid_params().data(format!("Unknown session {session_id}: {e}")))
}

/// What the validation needs to know about the chat: its working dir, and whether its model is a
/// swarm build (every tick would start a full build).
pub fn chat_facts(session: &Session) -> ChatFacts {
    let swarm_build = session.provider_name.as_deref() == Some(SWARM_PROVIDER)
        && session.model_config.as_ref().is_some_and(|m| {
            matches!(
                parse_route_model(&m.model_name),
                Some(RouteModel::Build | RouteModel::BuildStrategy { .. })
            )
        });
    ChatFacts {
        working_dir: session.working_dir.to_string_lossy().to_string(),
        swarm_build,
    }
}

fn proof_of(record: &LoopRecord) -> Option<OwnerProof> {
    record.owner.as_ref().map(seam::prove_owner)
}

/// Q-279: a yield records the chat's name at that moment, and a new chat is "New Chat" until its
/// first turn names it; a `user_turn` reason read now names the chat as it is now. A chat that can
/// no longer be read keeps the recorded name, and the log says so.
pub async fn turn_chat_named_now(
    session_manager: &SessionManager,
    mut reason: Option<LoopStatusReason>,
) -> Option<LoopStatusReason> {
    if let Some(LoopStatusReason::UserTurn { session_id, chat }) = &mut reason {
        match session_manager.get_session(session_id, false).await {
            Ok(turn_chat) => *chat = turn_chat.name,
            Err(error) => tracing::warn!(
                session_id = %session_id,
                %error,
                "loop: the chat of a user turn could not be read; its name is the one recorded"
            ),
        }
    }
    reason
}

pub async fn get(
    session_manager: &SessionManager,
    req: LoopsGetRequest,
) -> Result<LoopsGetResponse, AcpError> {
    let session = session(session_manager, &req.session_id).await?;
    let stored = record::parse_stored(
        session
            .extension_data
            .get_extension_state(record::EXTENSION_NAME, record::VERSION),
    );
    Ok(match stored {
        Ok(None) => LoopsGetResponse::default(),
        Ok(Some(record)) => {
            let (status, reason) = rules::effective_status(&record, proof_of(&record).as_ref());
            let reason = turn_chat_named_now(session_manager, reason).await;
            LoopsGetResponse {
                record: Some(record),
                effective_status: Some(status),
                effective_reason: reason,
                error: None,
            }
        }
        Err(error) => LoopsGetResponse {
            error: Some(error),
            ..LoopsGetResponse::default()
        },
    })
}

/// Every chat's loop, ended ones included, each with its status as read now.
pub async fn list(session_manager: &SessionManager) -> Result<LoopsListResponse, AcpError> {
    let rows = record::read_all(session_manager).await.map_err(internal)?;
    let loops = rows
        .into_iter()
        .map(|(session_id, parsed)| match parsed {
            Ok(record) => {
                let (status, _) = rules::effective_status(&record, proof_of(&record).as_ref());
                let next_tick_at = match status {
                    LoopStatus::Waiting => record.next_tick.as_ref().map(|n| n.at.clone()),
                    _ => None,
                };
                LoopSummaryDto {
                    session_id,
                    status: Some(status),
                    next_tick_at,
                    error: None,
                }
            }
            Err(error) => LoopSummaryDto {
                session_id,
                status: None,
                next_tick_at: None,
                error: Some(error),
            },
        })
        .collect();
    Ok(LoopsListResponse { loops })
}

/// The chats whose loop has not ended, for `session_activity/get`'s `looping`.
pub async fn looping(session_manager: &SessionManager) -> Result<Vec<LoopSummaryDto>, AcpError> {
    Ok(list(session_manager)
        .await?
        .loops
        .into_iter()
        .filter(|summary| summary.status != Some(LoopStatus::Ended))
        .collect())
}

pub fn templates() -> LoopsTemplatesResponse {
    LoopsTemplatesResponse {
        templates: templates::all(),
    }
}

fn changed(result: Result<LoopRecord, LoopRefusal>) -> LoopsChangeResponse {
    match result {
        Ok(record) => LoopsChangeResponse {
            record: Some(record),
            refusal: None,
        },
        Err(refusal) => LoopsChangeResponse {
            record: None,
            refusal: Some(refusal),
        },
    }
}

fn start_edit(req: LoopsStartRequest) -> LoopEdit {
    LoopEdit {
        goal: req.goal,
        template: req.template,
        steps: req.steps,
        cadence: req.cadence,
        state_file: req.state_file,
        check: req.check,
        stop_after_ticks: req.stop_after_ticks,
    }
}

/// Validate, then start through the runner. Refused by name when the runner or the report tool is
/// not in this build (a loop without the tool would end every tick without a report).
pub async fn start(
    session_manager: &SessionManager,
    req: LoopsStartRequest,
) -> Result<LoopsChangeResponse, AcpError> {
    let runner = match seam::runner() {
        Ok(runner) => runner,
        Err(refusal) => return Ok(changed(Err(refusal))),
    };
    if !seam::extension_sync_installed() {
        return Ok(changed(Err(seam::extension_sync_absent())));
    }
    let session = session(session_manager, &req.session_id).await?;
    let session_id = req.session_id.clone();
    let valid = match rules::validate_loop(&start_edit(req), &chat_facts(&session)) {
        Ok(valid) => valid,
        Err(refusal) => return Ok(changed(Err(refusal))),
    };
    Ok(changed(runner.start(&session_id, valid).await))
}

pub async fn update(
    session_manager: &SessionManager,
    req: LoopsUpdateRequest,
) -> Result<LoopsChangeResponse, AcpError> {
    let runner = match seam::runner() {
        Ok(runner) => runner,
        Err(refusal) => return Ok(changed(Err(refusal))),
    };
    let session = session(session_manager, &req.session_id).await?;
    let valid = match rules::validate_loop(&req.patch, &chat_facts(&session)) {
        Ok(valid) => valid,
        Err(refusal) => return Ok(changed(Err(refusal))),
    };
    Ok(changed(runner.update(&req.session_id, valid).await))
}

pub async fn control(req: LoopsControlRequest) -> Result<LoopsChangeResponse, AcpError> {
    Ok(changed(match seam::runner() {
        Ok(runner) => runner.control(&req.session_id, req.action).await,
        Err(refusal) => Err(refusal),
    }))
}

pub async fn tick_refused(
    req: LoopsTickRefusedRequest,
) -> Result<LoopsTickRefusedResponse, AcpError> {
    let refusal = match seam::runner() {
        Ok(runner) => runner.tick_refused(req).await.err(),
        Err(refusal) => Some(refusal),
    };
    Ok(LoopsTickRefusedResponse { refusal })
}

pub async fn ready(req: LoopsReadyRequest) -> Result<LoopsReadyResponse, AcpError> {
    Ok(match seam::runner() {
        Ok(runner) => match runner.ready(&req.session_id).await {
            Ok(reoffered) => LoopsReadyResponse {
                reoffered,
                refusal: None,
            },
            Err(refusal) => LoopsReadyResponse {
                reoffered: false,
                refusal: Some(refusal),
            },
        },
        Err(refusal) => LoopsReadyResponse {
            reoffered: false,
            refusal: Some(refusal),
        },
    })
}

pub async fn wake() -> Result<LoopsWakeResponse, AcpError> {
    Ok(match seam::runner() {
        Ok(runner) => match runner.wake().await {
            Ok(rearmed) => LoopsWakeResponse {
                rearmed,
                refusal: None,
            },
            Err(refusal) => LoopsWakeResponse {
                rearmed: 0,
                refusal: Some(refusal),
            },
        },
        Err(refusal) => LoopsWakeResponse {
            rearmed: 0,
            refusal: Some(refusal),
        },
    })
}
