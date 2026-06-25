//! The model-agnostic dispatch boundary. The scheduler only ever runs a task by calling
//! [`TaskDispatcher::run`]; the real implementation (in goose-cli) drives a Goose Agent bound to a
//! device's LM Link model id, while tests use a mock. This keeps the concurrency core testable.

use async_trait::async_trait;

/// One unit of work handed to a device.
#[derive(Clone, Debug)]
pub struct DispatchRequest {
    pub task_id: String,
    pub description: String,
    /// The device this task was routed to (pool id).
    pub device_id: String,
    /// The LM Link model id LM Studio routes to the device.
    pub model_id: String,
    /// The relevant slice of shared context (dependency outputs) for this task.
    pub context_slice: String,
    /// 0-based attempt number (incremented on re-dispatch).
    pub attempt: u32,
}

/// Outcome of a failed dispatch. `Transient` is re-dispatched (and steered to a different device);
/// `Terminal` fails the task and its descendants.
#[derive(Clone, Debug)]
pub enum DispatchError {
    /// Recoverable — e.g. LM Studio "Model is unloaded". Re-dispatch within the attempt budget.
    Transient(String),
    /// Unrecoverable for this task.
    Terminal(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::Transient(m) => write!(f, "transient: {m}"),
            DispatchError::Terminal(m) => write!(f, "terminal: {m}"),
        }
    }
}

impl std::error::Error for DispatchError {}

#[async_trait]
pub trait TaskDispatcher: Send + Sync {
    /// Run one task on its assigned device. Returns the task's final output (e.g. the typed
    /// `final_output` payload) on success.
    async fn run(&self, req: DispatchRequest) -> Result<String, DispatchError>;
}
