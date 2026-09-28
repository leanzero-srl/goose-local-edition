//! Q-423: a Rapid-MLX stream that fails tells its client only "Internal error during streaming"
//! (the pinned fork sanitizes the exception out of the SSE error, helpers.py disconnect_guard) and
//! logs the exception on the engine's stderr — which goose-sidecar reads on the Mac that runs the
//! engine. On 2026-09-28 the Studio's single failed a 191,200-token answer after one token and the
//! turn said only "Server error: Internal error during streaming"; the exception lived nowhere
//! anyone could read it.
//!
//! [`EngineStreamErrors`] is taken as a request goes to THIS goosed's single engine and, when the
//! stream ends in those sanitized words, answers with what the engine logged for it — on the node
//! that logged it: the Link inference proxy's serving side for a linked Mac's chat
//! (`GoosedChatServing::stream_errors`), and the swarm router for this Mac's own engine
//! ([`EngineStreamErrors::explaining`]).

use std::sync::Arc;

use futures::StreamExt;
use goose_providers::errors::ProviderError;
use goose_sidecar::engine::{explain_stream_failure, global_manager, ENGINE_STREAM_FAILURE};
use goose_sidecar::ErrorMark;
use leanzero_link::state::StreamErrorExplainer;

use super::base::MessageStream;

/// Where this goosed's engine's logged errors stood as one request was sent to it.
pub struct EngineStreamErrors {
    mark: Option<ErrorMark>,
}

impl EngineStreamErrors {
    pub async fn mark() -> Self {
        Self {
            mark: global_manager().engine_error_mark().await,
        }
    }

    /// The engine's sanitized failure completed with what it logged; `None` for any other words.
    pub async fn explain(&self, message: &str) -> Option<String> {
        if message != ENGINE_STREAM_FAILURE {
            return None;
        }
        let explained =
            explain_stream_failure(global_manager().engine_error_since(self.mark).await);
        tracing::warn!(target: "mlx_engine", "a stream failed: {explained}");
        Some(explained)
    }

    /// `stream` with the engine's sanitized failure explained.
    pub fn explaining(self, stream: MessageStream) -> MessageStream {
        let errors = Arc::new(self);
        Box::pin(stream.then(move |item| {
            let errors = Arc::clone(&errors);
            async move {
                match item {
                    Err(ProviderError::ServerError(message)) => {
                        match errors.explain(&message).await {
                            Some(explained) => Err(ProviderError::ServerError(explained)),
                            None => Err(ProviderError::ServerError(message)),
                        }
                    }
                    other => other,
                }
            }
        }))
    }
}

#[async_trait::async_trait]
impl StreamErrorExplainer for EngineStreamErrors {
    async fn explain(&self, message: &str) -> Option<String> {
        EngineStreamErrors::explain(self, message).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With no engine running in this goosed, the sanitized words are still completed — with why
    /// the engine's account cannot be read — and any other error passes as the engine worded it.
    #[tokio::test]
    async fn the_sanitized_failure_is_never_left_bare() {
        let stream: MessageStream = Box::pin(futures::stream::iter(vec![
            Err(ProviderError::ServerError(
                ENGINE_STREAM_FAILURE.to_string(),
            )),
            Err(ProviderError::ServerError(
                "Request cancelled by model replacement".to_string(),
            )),
        ]));
        let items: Vec<_> = EngineStreamErrors { mark: None }
            .explaining(stream)
            .collect()
            .await;
        match &items[0] {
            Err(ProviderError::ServerError(said)) => assert_eq!(
                said,
                "Internal error during streaming — what the MLX engine logged for it cannot be \
                 read: no engine was running here when this answer was asked for"
            ),
            other => panic!("{other:?}"),
        }
        match &items[1] {
            Err(ProviderError::ServerError(said)) => {
                assert_eq!(said, "Request cancelled by model replacement")
            }
            other => panic!("{other:?}"),
        }
    }
}
