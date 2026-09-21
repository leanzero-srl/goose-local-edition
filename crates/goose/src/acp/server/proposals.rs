//! Memory proposals over ACP (frame 1.14 G5): list what the agent asked to remember for a
//! session, and record the human's answer. Save is the ONLY path from a proposal to a memory,
//! and it goes through `goose_memory_store::MemoryStore::remember` — never a second writer.

use goose_memory_store::{
    working_dir_key, MemoryProposal, MemoryStore, Polarity, ProposalKind, ProposalState,
    ProposalStore, RememberOutcome,
};
use goose_sdk_types::custom_requests::{
    AnswerMemoryProposalRequest, AnswerMemoryProposalResponse, ListMemoryProposalsRequest,
    ListMemoryProposalsResponse, MemoryProposalDecision, MemoryProposalDto, MemoryProposalKind,
    MemoryProposalPolarity, MemoryProposalState,
};

use super::{GooseAcpAgent, ResultExt};

fn dto(key: &str, p: &MemoryProposal) -> MemoryProposalDto {
    MemoryProposalDto {
        id: p.id.clone(),
        key: key.to_string(),
        kind: match p.kind {
            ProposalKind::Memory => MemoryProposalKind::Memory,
            ProposalKind::Knowledge => MemoryProposalKind::Knowledge,
        },
        polarity: p.polarity.map(|pol| match pol {
            Polarity::Positive => MemoryProposalPolarity::Positive,
            Polarity::Negative => MemoryProposalPolarity::Negative,
        }),
        text: p.text.clone(),
        why: p.why.clone(),
        category: p.category.clone(),
        tags: p.tags.clone(),
        is_global: p.is_global,
        sources: p.sources.clone(),
        created_at: p.created_at,
        state: match p.state {
            ProposalState::Open => MemoryProposalState::Open,
            ProposalState::Saved => MemoryProposalState::Saved,
            ProposalState::Declined => MemoryProposalState::Declined,
            ProposalState::Expired => MemoryProposalState::Expired,
        },
    }
}

impl GooseAcpAgent {
    fn proposal_store(&self) -> ProposalStore {
        ProposalStore::new(self.config_dir.join("proposals"))
    }

    pub(super) async fn on_list_memory_proposals(
        &self,
        req: ListMemoryProposalsRequest,
    ) -> Result<ListMemoryProposalsResponse, agent_client_protocol::Error> {
        let session = self
            .session_manager
            .get_session(&req.session_id, false)
            .await
            .internal_err_ctx("Failed to load session")?;
        let store = self.proposal_store();
        let wd_key = working_dir_key(&session.working_dir);
        let mut proposals = Vec::new();
        for key in [req.session_id.as_str(), wd_key.as_str()] {
            // A missing or unreadable file is an honest empty: a KVS blip must not blank the
            // transcript (fail open on reads).
            let rows = store.list(key).unwrap_or_default();
            proposals.extend(rows.iter().map(|p| dto(key, p)));
        }
        proposals.sort_by_key(|p| p.created_at);
        Ok(ListMemoryProposalsResponse { proposals })
    }

    pub(super) async fn on_answer_memory_proposal(
        &self,
        req: AnswerMemoryProposalRequest,
    ) -> Result<AnswerMemoryProposalResponse, agent_client_protocol::Error> {
        let session = self
            .session_manager
            .get_session(&req.session_id, false)
            .await
            .internal_err_ctx("Failed to load session")?;
        let wd_key = working_dir_key(&session.working_dir);
        if req.key != req.session_id && req.key != wd_key {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("proposal key does not belong to this session"));
        }
        let store = self.proposal_store();
        let saved = matches!(req.decision, MemoryProposalDecision::Save);
        let Some(answered) = store
            .answer(&req.key, &req.proposal_id, saved, req.text.as_deref())
            .internal_err_ctx("Failed to record the answer")?
        else {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("proposal is not open (unknown id, already answered, or expired)"));
        };
        let mut outcome = None;
        if saved {
            let memory_store =
                MemoryStore::new(self.config_dir.join("memory"), &session.working_dir);
            let result = memory_store
                .remember(
                    &answered.category,
                    &answered.text,
                    &answered.tags,
                    answered.is_global,
                )
                .internal_err_ctx("Failed to write the memory")?;
            outcome = Some(
                match result {
                    RememberOutcome::Added => "added",
                    RememberOutcome::Updated => "updated",
                    RememberOutcome::Unchanged => "unchanged",
                }
                .to_string(),
            );
        }
        Ok(AnswerMemoryProposalResponse {
            proposal: Some(dto(&req.key, &answered)),
            outcome,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire shape carries every field the card renders, and the store's enums map one to
    /// one — an out-of-enum value cannot exist on either side.
    #[test]
    fn a_stored_proposal_maps_to_the_wire_shape_field_for_field() {
        let p = MemoryProposal {
            id: "p-1-1".into(),
            kind: ProposalKind::Memory,
            polarity: Some(Polarity::Negative),
            text: "the retry lives in src/http/retry.js".into(),
            why: "repeated correction".into(),
            category: "lessons".into(),
            tags: vec!["feedback".into()],
            is_global: false,
            sources: vec![],
            created_at: 7,
            state: ProposalState::Open,
        };
        let d = dto("sess-1", &p);
        assert_eq!(d.key, "sess-1");
        assert_eq!(d.kind, MemoryProposalKind::Memory);
        assert_eq!(d.polarity, Some(MemoryProposalPolarity::Negative));
        assert_eq!(d.state, MemoryProposalState::Open);
        assert_eq!(d.text, p.text);
        assert_eq!(d.why, p.why);
        assert_eq!(d.category, "lessons");
        assert_eq!(d.created_at, 7);
    }
}
