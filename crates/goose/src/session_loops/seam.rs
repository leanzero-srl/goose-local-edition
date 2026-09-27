//! The seam (design §9 L0): how the `loops/*` handlers, the needs-you resolve handler and the
//! agent reach the loop runner (L2a) and the report tool's extension sync (L3) without L0
//! depending on either. The shape of `nodes::seam`: a trait, one install point each, and — with
//! nothing installed — a NAMED answer, never a silent default. Every mutation refuses with
//! "The loop runner is not in this build"; a start refuses by name too when the runner is there
//! but the report tool is not, since every tick would then end without a report.

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use goose_sdk_types::custom_requests::{
    LoopControlAction, LoopEdit, LoopOwner, LoopRecord, LoopRefusal, LoopRefusalCode,
    LoopsTickRefusedRequest,
};

use super::rules::OwnerProof;
use crate::agents::Agent;

pub const RUNNER_ABSENT: &str = "The loop runner is not in this build";
pub const EXTENSION_SYNC_ABSENT: &str = "The loop report tool is not in this build";

/// The process-wide loop runner (L2a): it owns the record's writes, the clock and every decision.
/// Every edit it receives is already validated (`rules::validate_loop`).
#[async_trait]
pub trait LoopRunner: Send + Sync {
    /// Start (or replace) the chat's loop and claim its clock; the first tick is offered now.
    async fn start(&self, session_id: &str, start: LoopEdit) -> Result<LoopRecord, LoopRefusal>;
    async fn update(&self, session_id: &str, edit: LoopEdit) -> Result<LoopRecord, LoopRefusal>;
    async fn control(
        &self,
        session_id: &str,
        action: LoopControlAction,
    ) -> Result<LoopRecord, LoopRefusal>;
    async fn tick_refused(&self, refused: LoopsTickRefusedRequest) -> Result<(), LoopRefusal>;
    /// `true` = an open offer was re-sent.
    async fn ready(&self, session_id: &str) -> Result<bool, LoopRefusal>;
    /// The number of loops whose next tick was re-armed from the wall clock.
    async fn wake(&self) -> Result<u32, LoopRefusal>;
    /// Whether the recorded owner is this process, another live one, or proven gone.
    fn prove_owner(&self, owner: &LoopOwner) -> OwnerProof;
    /// A needs-you item of this session was answered or dismissed (§4.6's asked rule).
    fn needs_you_resolved(&self, session_id: &str, item_id: &str);
}

/// Makes the session's agent carry the `loop_report` tool exactly while the loop has not ended
/// (L3's `agent_sync::sync_loop_extension`); idempotent.
#[async_trait]
pub trait LoopExtensionSync: Send + Sync {
    async fn sync(
        &self,
        agent: Arc<Agent>,
        session_id: &str,
        record: &LoopRecord,
    ) -> Result<(), String>;
}

static RUNNER: OnceLock<Arc<dyn LoopRunner>> = OnceLock::new();
static EXTENSION_SYNC: OnceLock<Arc<dyn LoopExtensionSync>> = OnceLock::new();

/// Install this process's runner. The first install wins (one runner per process).
pub fn install_runner(runner: Arc<dyn LoopRunner>) -> bool {
    RUNNER.set(runner).is_ok()
}

pub fn install_extension_sync(sync: Arc<dyn LoopExtensionSync>) -> bool {
    EXTENSION_SYNC.set(sync).is_ok()
}

pub fn runner_installed() -> bool {
    RUNNER.get().is_some()
}

pub fn extension_sync_installed() -> bool {
    EXTENSION_SYNC.get().is_some()
}

pub fn runner_absent() -> LoopRefusal {
    LoopRefusal {
        code: LoopRefusalCode::RunnerAbsent,
        reason: RUNNER_ABSENT.to_string(),
    }
}

pub fn extension_sync_absent() -> LoopRefusal {
    LoopRefusal {
        code: LoopRefusalCode::ExtensionSyncAbsent,
        reason: EXTENSION_SYNC_ABSENT.to_string(),
    }
}

/// The installed runner, or the named refusal.
pub fn runner() -> Result<Arc<dyn LoopRunner>, LoopRefusal> {
    RUNNER.get().cloned().ok_or_else(runner_absent)
}

/// With no runner, nothing in this process can prove an owner live or gone.
pub fn prove_owner(owner: &LoopOwner) -> OwnerProof {
    match RUNNER.get() {
        Some(runner) => runner.prove_owner(owner),
        None => OwnerProof::Unproven {
            why: RUNNER_ABSENT.to_string(),
        },
    }
}

/// With no runner no loop runs in this process, so no loop waits on the item.
pub fn needs_you_resolved(session_id: &str, item_id: &str) {
    if let Some(runner) = RUNNER.get() {
        runner.needs_you_resolved(session_id, item_id);
    }
}

pub async fn sync_loop_extension(
    agent: Arc<Agent>,
    session_id: &str,
    record: &LoopRecord,
) -> Result<(), String> {
    match EXTENSION_SYNC.get() {
        Some(sync) => sync.sync(agent, session_id, record).await,
        None => Err(EXTENSION_SYNC_ABSENT.to_string()),
    }
}
