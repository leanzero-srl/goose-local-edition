//! Session activity over ACP: which sessions have a turn in flight and which are waiting on the
//! person (`ask_user` items), and closing an item with the person's answer or a dismissal.

use std::collections::HashMap;

use goose_sdk_types::custom_requests::{
    FailedSessionDto, NeedsYouAction, NeedsYouItemDto, NeedsYouStatus as NeedsYouStatusDto,
    ResolveNeedsYouRequest, ResolveNeedsYouResponse, RunningSessionDto, SessionActivityRequest,
    SessionActivityResponse,
};
use tracing::warn;

use super::{GooseAcpAgent, ResultExt};
use crate::execution::manager::AgentManager;
use crate::needs_you::{self, NeedsYouItem, NeedsYouStatus, Resolution};
use crate::session::SessionType;

fn dto(
    session_id: &str,
    session_name: &str,
    working_dir: &std::path::Path,
    item: NeedsYouItem,
) -> NeedsYouItemDto {
    NeedsYouItemDto {
        id: item.id,
        session_id: session_id.to_string(),
        session_name: session_name.to_string(),
        working_dir: working_dir.to_string_lossy().to_string(),
        question: item.question,
        why: item.why,
        recommended_answer: item.recommended_answer,
        options: item.options,
        created_at: item.created_at.to_rfc3339(),
        status: match item.status {
            NeedsYouStatus::Open => NeedsYouStatusDto::Open,
            NeedsYouStatus::Answered => NeedsYouStatusDto::Answered,
            NeedsYouStatus::Dismissed => NeedsYouStatusDto::Dismissed,
        },
        answer: item.answer,
    }
}

impl GooseAcpAgent {
    /// The busy set of this process: the ACP server's own manager and, when goose-server built
    /// one, the process singleton (the same union LeanZero Link reads). Earliest start wins.
    async fn busy_sessions(&self) -> HashMap<String, chrono::DateTime<chrono::Utc>> {
        let mut busy: HashMap<String, chrono::DateTime<chrono::Utc>> = HashMap::new();
        let mut managers = vec![self.agent_manager.clone()];
        if let Some(shared) = AgentManager::instance_if_built() {
            if !std::sync::Arc::ptr_eq(&shared, &self.agent_manager) {
                managers.push(shared);
            }
        }
        for manager in managers {
            for (session_id, since) in manager.busy_sessions().await {
                busy.entry(session_id)
                    .and_modify(|earliest| *earliest = (*earliest).min(since))
                    .or_insert(since);
            }
        }
        busy
    }

    pub(super) async fn on_session_activity(
        &self,
        _req: SessionActivityRequest,
    ) -> Result<SessionActivityResponse, agent_client_protocol::Error> {
        let mut running = Vec::new();
        for (session_id, since) in self.busy_sessions().await {
            match self.session_manager.get_session(&session_id, false).await {
                Ok(session)
                    if matches!(
                        session.session_type,
                        SessionType::User | SessionType::Scheduled
                    ) =>
                {
                    running.push(RunningSessionDto {
                        session_id,
                        session_name: session.name,
                        working_dir: session.working_dir.to_string_lossy().to_string(),
                        started_at: since.to_rfc3339(),
                    });
                }
                Ok(_) => {}
                Err(error) => {
                    warn!(%session_id, %error, "a busy session is unreadable in the store");
                }
            }
        }
        running.sort_by(|a, b| a.started_at.cmp(&b.started_at));

        let needs_you = needs_you::open_items(&self.session_manager)
            .await
            .internal_err_ctx("Failed to list needs-you items")?
            .into_iter()
            .map(|open| {
                dto(
                    &open.session_id,
                    &open.session_name,
                    &open.working_dir,
                    open.item,
                )
            })
            .collect();

        let failed = crate::turn_outcome::failed_sessions(&self.session_manager)
            .await
            .internal_err_ctx("Failed to list failed sessions")?
            .into_iter()
            .map(|failed| FailedSessionDto {
                session_id: failed.session_id,
                session_name: failed.session_name,
                working_dir: failed.working_dir.to_string_lossy().to_string(),
                failed_at: failed.at.to_rfc3339(),
                reason: failed.reason,
            })
            .collect();
        Ok(SessionActivityResponse {
            running,
            needs_you,
            failed,
        })
    }

    pub(super) async fn on_resolve_needs_you(
        &self,
        req: ResolveNeedsYouRequest,
    ) -> Result<ResolveNeedsYouResponse, agent_client_protocol::Error> {
        let resolution = match req.action {
            NeedsYouAction::Answer => Resolution::Answered(req.answer.ok_or_else(|| {
                agent_client_protocol::Error::invalid_params().data("An answer needs `answer`")
            })?),
            NeedsYouAction::Dismiss => Resolution::Dismissed,
        };
        let session = self
            .session_manager
            .get_session(&req.session_id, false)
            .await
            .invalid_params_err_ctx("Unknown session")?;
        let item = needs_you::resolve(
            &self.session_manager,
            &req.session_id,
            &req.item_id,
            resolution,
        )
        .await
        .invalid_params_err()?;
        Ok(ResolveNeedsYouResponse {
            item: dto(&session.id, &session.name, &session.working_dir, item),
        })
    }
}
